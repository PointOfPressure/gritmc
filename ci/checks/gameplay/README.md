# The gameplay corpus

One TOML file per check. A check says what to **do**; it never says what
should happen — that comes from running the same file against real vanilla.
Adding a check is therefore writing down a sequence of actions, which is what
makes a corpus affordable to grow.

Every coordinate is relative to the **anchor**: the block the player is
standing in. The substrate is vanilla's own superflat, so the ground is at
`[_, -1, _]` and the air the player occupies is `[0, 0, 0]`. Work beside the
player rather than inside them — vanilla refuses to place a block into the
space an entity occupies, and a scenario that does it measures the refusal
instead of the thing it meant to.

**Keep a rig on one side of relative `z = 0`.** The workspace origin sits at
absolute `(-(n+1) * 32, spawn_y, 0)`, which is *on* a chunk boundary: anything
at relative z >= 0 is in chunk z 0 and anything at z <= -1 is in chunk z -1.
Vanilla cannot carry two chunks in one `section_blocks_update`, so a rig that
writes on both sides gets two bare `block_update`s whose order is whatever the
per-chunk tracker produced that run — and the check is then measuring vanilla's
inter-chunk send order rather than the mechanic it names.

This has now cost three checks. 235 was a coin flip and was found by six
consecutive vanilla recordings; 360 was found by raising
`ci/t2-gameplay-selfcheck.sh` from two runs to twelve, and had the same shape;
410's own repair introduced a third and it was caught by scanning for the
pattern rather than by another run. All three were closed by offsetting the
whole rig on +z, which changes nothing about what the check asks. A scan of the
corpus for rigs spanning z = 0 currently leaves one,
`390-a-piston-pushes-the-block-in-front`, which has not been observed to flap.

**A check whose mechanism repeats on a server timer should declare
`collapse_repeats`.** A run of identical *consecutive* observations of a named
kind then compares as one. It exists for exactly one shape: a mechanism that
re-emits the same packet for as long as a state holds, so that how many arrive
inside `settle_ms` is decided by where the window edge falls and not by either
server. `090` and `490` are the two — an open chest signals its lid every 250 ms
for ever — and without it the repeat count *was* the check, and vanilla
disagreed with itself about it at 1/25 and 3/25.

Two rules come with it and neither is optional. **The kind must be in
`observe` or in `trace::DECODED_KINDS`** — `Scenario::load` refuses anything
else, because a declaration that cannot bite is one nobody will notice is
dead.

*This paragraph said "must be in `observe`" until 2026-08-10, and eleven
checks in this directory contradicted it.* `560`, `561` and `601`-`608` all
declare `collapse_repeats = ["block_event"]` without observing `block_event`,
and every one of them loads. They are right and the sentence was wrong: the
**strict** denominator ignores `observe` entirely, so a chest lid signalling
every 250 ms destabilises the strict projection of a check whose declared
projection never mentions it. Collapsing it there is the whole point, and
requiring the kind in `observe` would have forced ten container checks to
widen their declared projection to fix a strict one. The loader's own test is
`!observe.contains(k) && !DECODED_KINDS.contains(k)`.

And **the coverage it removes has to go somewhere**: the check stops testing the cadence, so a server that
signalled once and never repeated would pass, and the gap (not the count) must
be claimed by a unit test in the same change. See ORACLE.md rule 26.

**The other kind of exemption is invisible from this directory, and you have to
know it exists.** A field can also be dropped by `trace::decode` *consuming*
it, so it never reaches `rest` and no check anywhere can see it. Three are
exempted that way today: `add_entity`'s uuid, `sound`'s seed, and — since
2026-08-05 — `add_entity`'s **movement**,
`ServerEntity.getLastSentMovement()`, which is a coin toss vanilla makes for a
standing player and four unseeded `nextFloat()` draws for a thrown item.

Nothing in a `.toml` records that, so **a check file cannot tell you what its
own projection excludes.** If a field you expect to be compared is not moving
a verdict, read the arm in `trace::decode` before concluding the servers
agree. The same two rules apply — the coverage has to go somewhere, and what
stays unmeasured gets written down — but the cheap saved-recording control
does **not**, because the decoder runs at record time and the digest it would
have to undo has already been taken. ORACLE rule 26 has the split and the
control set that replaces it.

**A check that observes `block_update` must observe
`section_blocks_update` too.** Vanilla coalesces more than one change in one
section in one tick into a single `section_blocks_update`, so every mechanic
that moves two blocks at once — a door, a bed, a pair of chests, a fence
connecting, a torch falling off its support — arrives as that packet and not
as two `block_update`s. Eight checks were reported `inert` on 2026-08-03 for
want of it: vanilla had spoken, the projection could not hear it, and the
check could not fail.

**Some checks exist because another one could not fail.** A check exercises
one input, and a mechanism that reads something *from* that input can be
faked by a constant which the single input never distinguishes:

```text
  230-bed-places-two-blocks          places one bed, facing south, head
                                     always at [1,0,1]
  235-a-bed-facing-north-...         the same placement, player turned round

  330-button-unpresses-itself        stone button, 20 ticks
  335-a-wooden-button-stays-...      oak button, 30 ticks
```

A server that hard-codes `[1,0,1]` passes 230 for ever, and hard-coding is the
*natural* mistake here because a door's second block really is always `above`.
One that writes 20 down once passes 330 for ever. The second of each pair
differs in exactly the property being claimed and nothing else. When you add a
check for a mechanism that reads a value, ask which value the check pins — and
if it pins only one, the check cannot tell the mechanism from a constant.

**A third way a check can be unable to fail, and it looks like success.** Some
serverbound packets carry the *client's prediction of the result*, and the
server answers only where the prediction is wrong. `container_click` is the one
in this corpus: `broadcastChanges` sends a `container_set_slot` only for slots
whose truth differs from what the client is believed to think. So a harness
that predicted correctly would be answered with **silence**, both servers would
say nothing, and every storing and crafting check would report `match` against
a server with no containers. The harness therefore claims nothing — an empty
`changedSlots` and an empty carried item — and that is load-bearing rather than
lazy. If you add a verb for a packet of this shape, ask whether the server's
reply depends on what you claimed; if it does, a green on the first run is
evidence about the harness. ORACLE.md rule 27.

**Two mechanisms cannot be checked from this substrate at all, and the reason
is worth having written down before somebody spends a run on it.**

*Survival mining.* `Step::FinishDig` — `STOP_DESTROY_BLOCK`, the packet a
survival client sends when its own mining progress reaches one — is declared,
is correctly numbered since its ordinal was fixed on 2026-08-03, and is used
by **zero** checks. `./ci/corpus-coverage.py` prints that zero. It is not an
oversight and another scenario will not close it. The substrate is creative,
and in creative `ServerPlayerGameMode.handleBlockBreakAction` takes the
`instabuild` branch on `START_DESTROY_BLOCK` and **returns before
`destroyPos` is ever assigned** — that assignment is on the survival path, at
the "actual start of destroying" log line. A later `STOP_DESTROY_BLOCK` tests
`pos.equals(this.destroyPos)`, never matches, and falls through to a
`debugLogging` call that puts nothing on the wire. The one packet that does
arrive is the `ackBlockChangesUpTo(seq)` that
`ServerGamePacketListenerImpl.handlePlayerAction` makes unconditionally after
the game-mode call returns — no branch between the two, so it is sent whatever
the game mode did with the action. That ack is exactly what the scenario's own
action forced, which is the definition of `inert`, so a `finish_dig` check
would be refused by the gate rather than measure anything. Closing this needs
a **survival player**, which is a substrate change and not a check.

That is measured and not only read. `./ci/finish-dig-probe.sh` boots its own
vanilla, records a `finish_dig` against a block it has just placed, and
records a plain `dig` beside it as the control — because silence only means
something next to a rig known to be live. 2026-08-04:

```text
  probe-finish-dig-alone   block_changed_ack seq=3       and nothing else
  probe-dig-only           block_update [1,0,0] -> air, block_changed_ack
```

The stone is still standing after the `finish_dig`. The control removes it.

*The two remaining `setPlacedBy` second-block classes.* `SmallDripleafBlock`
and `TallSeagrassBlock` both write a second block at `[0, 1, 0]` and neither
has a placement check; `./ci/corpus-coverage.py` names them. Both need
somewhere to stand that vanilla's superflat does not provide, and the rules
are read rather than assumed. `SmallDripleafBlock.mayPlaceOn` is

```text
  state.is(BlockTags.SUPPORTS_SMALL_DRIPLEAF)
  || (level.getFluidState(pos.above()).isSourceOfType(Fluids.WATER)
      && super.mayPlaceOn(state, level, pos))
```

and the tag, from the jar's own datapack, is exactly `minecraft:clay` and
`minecraft:moss_block`. Superflat's top block is `grass_block` and there is no
water above it, so both arms are false. The seagrass wants water for the same
kind of reason. A check for either is therefore a **substrate** item and not a
corpus item — which is worth writing down, because the natural next move on
seeing a coverage zero is to write the scenario, and that scenario would place
nothing and record `inert`. The other five of the seven are placement-tested.

**Some checks cannot be closed on their own, because the packet they observe
carries a counter that runs for the whole session.** This is measured, from
the gate's own vanilla recording of 2026-08-04, and it changes which items
are cheap:

```text
  open_screen        window=1  chest-opens-a-screen
                     window=2  a-furnace-opens-a-screen-a-chest-does-not
                     window=3  a-crafting-table-opens-a-menu-with-no-block-entity
                     window=4  a-bystander-sees-a-chest-open

  container_set_slot window=0 state=2  torch-falls-with-its-support
                     window=0 state=3  swapping-to-the-offhand-... (slot 36)
                     window=0 state=4  swapping-to-the-offhand-... (slot 45)
```

`ServerPlayer.nextContainerCounter` is `containerCounter % 100 + 1` on a field
that lives as long as the player, and one actor plays every scenario in a
corpus run, so the window id in the third menu check is a **statement about
the two before it**. A server that implements only the crafting table sends
`window=1` where vanilla sends `window=3` and diverges on a field that has
nothing to do with crafting tables. The four menu checks are therefore one
indivisible item, in corpus order, and no subset of them can reach `match`.

`AbstractContainerMenu.incrementStateId` is the same shape for the player's
own inventory menu, which is `window=0` and never closes: the offhand swap
can only carry states 3 and 4 on a server that also produced state 2 where
`torch-falls-with-its-support` did.

> **THE `state` HALF OF THIS IS FIXED AND NO LONGER CONSTRAINS ANYTHING —
> 2026-08-11.** `compare::rebase_session_counters` rebases
> `container_set_slot.state` onto the scenario, scoped per
> `(connection, window)`, so what is compared is each packet's distance from
> the first one on that menu *in the check's own projection*. The paragraph
> above is still true of **vanilla** — the counter really does run for the
> whole session — and is no longer true of **the comparison**. A check
> observing `container_set_slot` no longer inherits anything from the checks
> before it through this field, and the offhand swap does not need
> `torch-falls-with-its-support` to have produced state 2.
>
> The `window` half of the block above was rebased earlier, by the
> `worldgen-iceberg-912` repair — read that function's own
> "closes the hazard `menu.rs` was already warning about" paragraph rather
> than taking it from here — so the "four menu checks are one indivisible
> item" sentence is history and not a current constraint.
>
> **AND THAT IS NOW MEASURED RATHER THAN READ — 2026-08-12.** Everything
> above, the claim and its retraction alike, was a reading of source, and a
> branch (`gameplay-block-classes`) refused a 17-block port on the strength of
> one. `ci/window-coupling.py` compares a real 109-scenario vanilla recording
> against a copy of itself with the whole session counter shifted, so the
> baseline is silent by construction: at +1, +2, +7 and +99 it moves **zero**
> verdicts across the 34 scenarios that carry an allocated window id. The four
> container-id defects it injects as controls — answer on window 0, name a
> menu one id off, never send `open_screen`, never increment — are caught by
> 33 checks, 33, 33 and **0** respectively. That last zero is a real hole and
> `757` is the scenario that closes it: two menus inside one scenario make the
> ids 1 and 2 a within-scenario fact. `docs/PARITY-ROADMAP.md` §42.
>
> What is
> **not** rebased and still binds is anything a scenario leaves in the
> *world* or in the actor's inventory — see the `790` rule at the end of this
> file, which survives on the free-slot layout alone.
>
> Three `ci/checks/gameplay-unstable.toml` entries existed only because of
> this field and all three were retired with the decoder; see that file.

This is a property of the *mechanism*, not a flaw in the checks — vanilla
really does number menus per player — and the right response is to schedule
the work in corpus order rather than to weaken the projection. It is written
here because the natural way to pick the next item is "which absent check
needs the least", and that ranking is wrong without it: on this corpus the
only `absent` check that is genuinely self-contained is
`server-answers-a-command`.

Checks for mechanics nobody has built yet are **wanted**, not premature. They
record as `absent`, which is the honest description of an unfinished server
and is also the worklist. `ci/checks/gameplay-baseline.toml` is the ratchet:
a check may not go backwards, and a new check must state where it starts.

The baseline lives one directory **up**, and that is not arbitrary: every
`.toml` in this directory is loaded as a scenario, so a baseline kept here
was parsed as a check and failed the whole run with a TOML error pointing at
its own first section header.

`open/` holds checks that are written down and not run, because they were
measured against vanilla and proved to demonstrate nothing. See its README.

## Before adding a check, look at the denominator — `ci/corpus-surface.py`

Everything above tells you how to write a check that works. It does not tell
you which check is worth writing, and until 2026-08-10 nothing did: the only
coverage instrument was `ci/corpus-coverage.py`, which counts our own sets
against each other. That is the inside question. **100% of a small set reads
exactly like 100% of a large one**, and this corpus reached the point where
every scenario it could decide already passed.

`./ci/corpus-surface.py` reads the pinned jar's own `GameProtocols`
registration for the 141 clientbound and 69 serverbound packet types, and
prints the set difference against what we name, what `trace.rs` can decode,
and what any check projects — plus, from recordings that already exist, what
vanilla **actually emitted** while the corpus ran. That last column is the
denominator worth using: two thirds of the protocol is unreachable from a
creative superflat with two players and no mobs, so counting boss-bar packets
as gaps is arithmetic, not information.

Three things it will tell you that are not visible from this directory:

* **which packets vanilla sends here that no check looks at.** Eight of them
  on 2026-08-10, five opaque. These need no new scenario and no new verb —
  the recording already exists and nothing reads it.
* **whether a candidate projection is stable, before you write the check.**
  `--recordings` reports the per-scenario occurrence count of every kind in
  every recording on the box. A kind vanilla already emits has already been
  sampled once per recording, so its rate is free. It has already ruled one
  candidate out: `rotate_head` is stable in three rigs and present in 9 of 17,
  2 of 17 and 16 of 17 runs of three others, so "observe `rotate_head`" is not
  a property of the kind but of how much the rig moves.
* **which block behaviours are untested, counted as implementations.** The
  jar-derived tables record each block's *runtime* class, so `TrappedChestBlock`
  and `CopperChestBlock` look like two uncovered classes when neither declares
  `useWithoutItem` — both inherit `ChestBlock`'s, which `090` covers. The
  script walks the superclass chain and collapses the column. 15 of 39
  implementations, not 18 of 47 (2026-08-12; it read 11 of 39 when written,
  and one of the four it gained is `redstone_wire`, which the corpus had been
  placing all along and scoring as uncovered — `docs/PARITY-ROADMAP.md` §39).
  **Quote a third figure beside it, not instead of it:** an implementation is
  only a unit of coverage if its members behave alike, and for
  `useWithoutItem` they do — 39 classes hold 40 distinct behaviours, so the
  collapse above is honest. For `useItemOn` they do not, and `ci/block-class-predicts.py`
  is the script that tells you which case you are in.

## A check that leaves a live mob behind is a check about the checks after it

`788` and `789` build golems, and a golem walks. That is a different
cross-scenario coupling from the two this file already warns about — it is not
a counter that runs for the session and it is not something left in the
actor's inventory; it is a thing in the world that **moves after the window
closes**.

**This paragraph used to say `778`, `779` and `780`, and used to say the
coupling was bounded. Both were wrong, and the correction cost a scenario.**
At N = 2 `ci/entity-spawn-probe.sh` caught `778`'s snow golem laying its snow
trail inside the *next* scenario's recording at 21 to 26 blocks from that
scenario's anchor — outside the default `within` of 16, with five blocks to
spare, and that was written down here as containment. At N = 6 the same probe
caught the same trail **inside** the copper golem's projection, at x 14..17,
and the copper golem flapped 5 pairs in 15 for no reason of its own. The five
blocks of spare were the width of one run's random walk. `778` is not in this
directory any more: `docs/PARITY-ROADMAP.md` §30.4 has the disposition, and the
short version is that a snow golem lays blocks and there is no number you can
give it that makes that safe.

So the mitigation is still ordering and it is now the *whole* mitigation: the
golems are the last checks before `790`, above
`780-random-ticks-turn-covered-grass-to-dirt`, and `790` is three workspaces
further on. **A margin you measured once is not a bound.** If you add a check
that leaves a live entity behind, number it after them, say in its header what
you measured about how far the thing travels — and say at what N, because this
paragraph is what a measurement at N = 2 reads like when it is wrong. An entity
that is merely announced and then sits still (`773`-`777`) has none of this
problem and can be numbered anywhere.

### The number is a shared namespace and it has now collided three times

These two checks have been renumbered **three times** and only the first was
their own doing: `730`-`737` -> `773`-`780` when the family was written,
`778`-`780` -> `781`-`782` when `780-random-ticks-turn-covered-grass-to-dirt`
landed on main while the branch was out, and `781`-`782` -> `788`-`789` when
`781-a-new-item-reaches-the-cursor-and-the-census` and its partner landed the
same way.

**The third collision was not cosmetic and that is what makes it worth a
paragraph.** `Scenario::load_dir` sorts by path, so
`781-a-carved-pumpkin-...` sorts *before* `781-a-new-item-...`: landing the
golems on their old numbers would have inserted a live, wandering mob **between
`781` and `782`**, which are a matched control pair whose whole argument is
that they differ in exactly one thing. A duplicate number is not a filename
nuisance; it is an insertion at a position nobody chose.

So before taking a number, take it against **every branch**, not against main:

```sh
for b in $(git branch -r | sed 's/^ *//' | grep -v HEAD); do
    git ls-tree --name-only "$b" ci/checks/gameplay/
done | sed 's|.*/||' | sort -u
```

On 2026-08-18 that showed `783`/`784` (`server-unported-audit`), `785`
(`proto-layer`), `787` (`trace-chunk-block-entities`) and `788`
(`world-send-chunk-block-entities`) all claimed by live branches with real
backlogs, leaving `786` and `789` as the only free slots below `790` — which is
not two slots for a *pair* that has to be last, because `787`/`788` would sit
between them. The golems therefore take `788`/`789`, and the one contested
number costs nothing: `787` and `788` are the **same check** under two branch
names, and the `787` variant is already written.

`790` did not move, and moving it was considered and rejected. It is the anchor
of a rule quoted in `docs/SERVER.md`, `tools/oracle-runner/src/compare.rs`,
three cold-start files and this README; `docs/cold-start/corpus-item-registry.md`
faced the identical choice on 2026-08-17 and settled it the same way —
**renumber the newcomer, never the anchor.**

### And for a check about a BLOCK rather than an interaction, the unit is the schema

`ci/corpus-surface.py` answers "which block *behaviours* are untested".
`ci/vanilla-reach-census.py`'s UNIT 3 answers "which blocks and block states
the corpus has ever caused to exist", and **one of its two rows must not be
used to pick a check.**

```text
  block_state         87 / 32366    0.27%     <- artefact, do not rank on it
  block_schema        31 /   116   26.72%     <- rank on this
```

For every one of the 1,196 blocks in the pinned jar, `len(states)` is exactly
the product of its property-domain sizes — zero exceptions — so 32,366 is a
Cartesian product and contains nothing beyond **116 distinct property-set
schemas** and how many blocks share each. A schema is one placement path and
one `updateShape`; two blocks with the same signature are the same work twice.
The 32 wall blocks are 10,368 states, **32.0% of the whole registry**, out of
two methods.

The practical consequence for someone about to write a check: **a scenario that
moves `block_state` a long way has usually moved `block_schema` by one or
zero**, and the second number is the one that says whether a mechanism was
added. `docs/PARITY-ROADMAP.md` §41 has the decomposition, the ranked list of
the 85 unobserved schemas, and the reason the obvious 1,196-block placement
sweep is refused rather than merely undone.

## The flower pot must stay the highest-numbered file in this directory

**It is `999-a-flower-pot-empties-when-right-clicked` as of 2026-08-13; it was
`790` when this section was written.** Branch `gameplay-block-schemas` was
allocated `960`-`999` by its brief, took the second arm of the rule at the end
of this section, and moved the pot to the top of the range. Its `name` did not
change, so nothing keyed by name moved with it. The rule below is unchanged in
force and the number in it is now the only thing that has ever changed.

`Scenario::load_dir` sorts by path, so corpus order is filename order, and
the flower pot check ends with a dandelion in the
actor's inventory: `FlowerPotBlock.useWithoutItem` calls `Player.addItem`.
That writes two things every later check inherits — the window-0
`incrementStateId` chain that the block above describes, and the free-slot
layout that decides where a later `hold` lands.

**One of those two is gone as of 2026-08-11 and the rule is unchanged in
force.** `compare::rebase_session_counters` rebases
`container_set_slot.state`, so the `incrementStateId` chain is no longer
something a later check inherits *in the comparison*. The free-slot layout is
not a counter, is rebased by nothing, and is enough on its own. Do not read
the first leg going as licence to renumber.

The cheapest correct answer is for nothing to come after it. **Number a new
check below the pot**, or move the pot up and say in its header that you did.

**And there is a second reason, which is the one that decided the 2026-08-13
move.** The pot is baselined `diverge` — its port is missing — so after it
runs, vanilla's inventory and ours are *already different*. A check placed
after it does not merely inherit a shifted free-slot layout; it inherits one
that differs **between the two servers**, and any difference that produces is
attributed to whatever mechanic the later check names. That is the
side-effects-reach-another-scenario case, which this corpus deletes checks for
rather than declaring, so "after the pot" is not a position a new check may
take even knowingly.
