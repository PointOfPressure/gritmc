### COLD START — branch `sounds`, off `main` (`b6237f29c`, 2,769), 2026-09-05

**The area: vanilla 26.2's 1,968 sound events and 125 particle types as tables
from the jar, the block sound types already wired here, and the `sound`,
`sound_entity`, `level_particles` and `level_event` packets against captures.**
Worktree `~/grit/work-sound2` (NOT `~/grit/work-sound`, which is an old checkout
at `c237f215a`), remote dir `~/rustmc-sound`.

#### What was on main, what is on the stack, what this adds

* **On main**: `crates/rustmc-server/src/sounds.rs` (the `sound` packet body,
  the exclusion, seven block-sound constructors, the chest's drawn pitch) and
  `sound_types.rs` (the five-event `SoundType` table, `data/vanilla/sound_types.json`).
  Two hand-written level-event ids and no table behind either. No particle code
  of any kind. No sound-event table, so no ranges.
* **On `origin/stack-gameplay` (305, landing)**: the sound-coverage figure
  (1,509 of 4,469 sounding states, 16 unreached), the pitch grading work, the
  click-surface sound work, `use_sound` for doors and trapdoors. **Taken as
  prior art and not duplicated** — `block.wooden_door.open` is deliberately
  still in this branch's gap list for that reason.
* **This branch adds** four generated tables, the three broadcast radii, three
  sound emitters, and the instrument that says where the rest of the work is.

#### The measurements

* **`./ci/sound-emission-gap.py`** is the instrument this area was missing. It
  reads every `oracle-work/gameplay/vanilla.json` on the box and asks which
  sound events vanilla *actually emitted* that this tree cannot produce — the
  complement of `figures.toml`'s 256-of-1,968 ceiling, which counts our own
  emitters. **32 recordings, 4,751 sound observations, 98 distinct events; 20
  beyond this tree at the start of the branch and 16 at the end.** It is a
  lower bound: its `.step` arm over-credits `entity.pig.step`, which is a mob's
  sound and not a block sound type. Ratcheted at `MAX_MISSING`.
* **No built-in sound event in 26.2 carries a fixed range — 0 of 1,968.** So
  every sound broadcasts at `volume > 1.0f ? 16.0f * volume : 16.0f`. Three
  agreeing derivations in `docs/FINDINGS.md`. This was found by a generator
  guard written the *other* way round, which failed on its first run.
* **The corpus has never carried a `level_particles` or a `sound_entity`
  packet**, and all 833 of its level events are `2001`. So a particle writer or
  an entity-sound writer landed today would be adjudicated by nothing.
* **125 particle types** (103 simple, 22 with options), **83 `LevelEvent`
  constants**, **27 `NoteBlockInstrument` values**, **0 events whose registry
  key differs from their sound location**.

#### What was ported

* **`ci/sound-particle-golden.sh`** + `tools/goldengen/vanilla/SoundParticleGolden.java`
  — four tables in one JVM run: `sound_events.json`, `particle_types.json`,
  `level_events.json`, `note_block.json`. Every count is asserted against
  Mojang's own `generated/reports/registries.json`, a different code path, and
  the whole sound name->id map entry for entry — which is what makes "the id
  this server writes is the registry id" a checkable property rather than two
  scripts happening to agree.
* **The three broadcast radii**, in `crates/rustmc-server/src/playerlist.rs`.
  `playSeededSound` uses `getRange(volume)`, `levelEvent` a constant 64.0 at
  the block's *raw* coordinates, and `sendParticles` — deliberately not shared
  — `closerToCenterThan(vec, longDistance ? 512 : 32)` from the player's block
  *centre*. All three comparisons are `dcmpg; ifge`, exclusive. Before this
  every sound and every level event went to every connected player;
  `sounds.rs`'s own header recorded what that cost on scenario 530 (vanilla 22
  step sounds on the observer's stream, ours 68).
* **Three emitters**: `block.piston.extend`/`.contract` (124 observations),
  `item.flintandsteel.use` (30), `block.note_block.harp` (60). The note block
  is the interesting one — it travels as a **block event**, so it lands behind
  its block update where every other block sound lands ahead; it is
  `SoundSource.RECORDS` and not `BLOCKS`; and at volume 3.0 it is the only
  thing in this server that reaches `getRange`'s multiply, so it carries 48
  blocks.
* **Two hand-written level-event ids are falsifiable now.**
  `shapes::LEVEL_EVENT_DESTROY` and `power::LEVEL_EVENT_REDSTONE_TORCH_BURNOUT`
  are checked against the jar's own constants. A wrong id there was a client
  playing some other event's particles at the right place.
* **`level_event_body` has a capture test** — both ints raw big-endian, not
  varints, against `010-break-block`'s recorded `event 2001, at [1,-1,0],
  data 9`.

#### Nulls — do not re-run these

* **Mob ambient sounds cannot be ported from the sound side.** 129 observations
  across four events, and this server does not tick mobs at all:
  `tick.rs`'s header says `rustmc_entity::tick` is derived and **nothing calls
  it**. Same refusal for `entity.player.swim` and `.splash`, which need a fluid
  state.
* **`*.hit` and `*.fall` block sounds are not missing call sites.** The
  mining-hit sound is played client-side by `LocalPlayer` and never crosses the
  wire; the fall sound needs `Entity.checkFallDamage`, which needs
  `Entity.move`. Neither appears anywhere in the corpus.
* **`SoundEvent.location()` never disagrees with the registry key.** 0 of
  1,968, despite the two-`Identifier` register overload that makes them
  separable. Nothing needs to carry the location.
* **A particle writer is not the next thing to build.** `particle_types.json`
  is in `DORMANT_GENERATORS` with that measurement as its reason.
* **A comment in our own source was refuted**: `itemuse.rs` said
  `item.flintandsteel.use` could not be heard in this corpus because the player
  is excluded. The exclusion is real and it excludes the *actor*; the corpus
  reads both streams and the event is on the observer's 30 times.

#### T0 on this branch: one red left, and it is not this branch's

`ci/order-selfconsistency.py` reds, and the reason is worth reading before
anybody spends a turn on it. Its `DEFAULT_GLOB` is
`~/rustmc*/oracle-work/gameplay*/vanilla.json` — **every tree on the box** — so
the scenario set it grades is whatever branches have recorded lately rather than
anything about the tree under test. The two rows the pin lacks are
`a-bat-left-far-away-despawns-and-a-pig-does-not` and
`a-spawned-pig-syncs-its-movement-speed-a-tick-later`; neither exists on this
branch or on `origin/main`, and `grep -l` finds them only in `~/rustmc-combat`,
`~/rustmc-mobs` and `~/rustmc-mobs2`. `ci/checks/order-selfconsistency.json` is
byte-identical between this branch's base and `origin/main`. **Every branch on
the box reds on it identically.** Deliberately not re-pinned from here — that
would bake three in-flight lanes' recordings into main's pin.

Everything else in T0 is green, including both gameplay ledgers.

#### What the corpus said, 2026-09-06

**99 of 109 declared match, 8 diverge, 2 absent, 0 vacuous, 0 mute, 0 inert,
0 error.** All three touched scenarios sweep to **tier 2, localised**, and
`380`'s `killed_by` now carries `perturb:sound`, `perturb:source` and
`perturb:volume`, which it structurally could not before its `observe` list
named `sound`.

* **`321-a-fire-being-lit-is-heard-by-everyone-else` opens at `match`** — the
  flint-and-steel sound is byte-exact on the observer's stream, which is the
  stream `320` cannot read.
* **`380`/`390` still diverge, and not on the sound**: both fail at the
  `piston_head` block update, which is the head appearing at all rather than
  what it sounds like.
* **`430` still diverges, and not on the sound either** — see the next section.

#### The single next item, and what closes it

**`data/vanilla/placement.json` classifies `minecraft:note_block` as
`yawOnly: true, neighbourSensitive: false`, and it is neither.** That is the
whole of `430`'s divergence and it is not a sound defect:
`NoteBlock.getStateForPlacement` is one line, offsets 0-16 —
`setInstrument(ctx.getLevel(), ctx.getClickedPos(), defaultBlockState())` —
which reads the block **above** and then the block **below** and touches no yaw
at all. The row carries **two** states, `instrument=harp` and
`instrument=basedrum`, which is the proof: a block whose placement really
depends only on yaw cannot produce two different instruments, so the probe saw
the variation and the classifier blamed the wrong cause. This server then picks
one by look direction and writes `basedrum` where the world says `harp`, at
observation 0, upstream of every kind `430` observes.

**It is not this area's to fix** — `tools/goldengen/vanilla/PlacementGolden.java`
and `crates/rustmc-server/src/placement.rs` belong to whoever owns placement —
and the sound behind it is already right: the `NOTE` cycle agrees with vanilla
on both sides.

It closes when `430`'s first observation agrees, which is when the note block's
instrument is read off its neighbours rather than off a yaw table. The sound
port then becomes testable through that check for the first time, and
`crate::noteblock` needs nothing further: it already reads the instrument off
whatever state the world holds.

Whoever takes it will need a per-block-state `instrument()` column, which is a
`BlockBehaviour.Properties` field exactly like `soundType` and which no table in
this tree carries. `ci/sound-particle-golden.sh` is the natural place for it —
it already walks `NoteBlockInstrument` — and adding it is about twenty lines of
`SoundParticleGolden.java`.
