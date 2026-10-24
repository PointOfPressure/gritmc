# Assessment, 2026-08-19: how complete this is, where it stands, and whether it finishes

Written while development is paused, from the repo, the pinned jar's own
generated reports, `cc-budget`, and the competitors' public claims. Nothing was
built, gated, recorded or harvested to produce it. One new instrument was
written, `ci/surface-census.py`, and it reads only the jar's reports and this
tree's source.

## The rule this document follows

Every number here names the question it answers, in the same sentence. That is
not a stylistic choice, it is the correction this project spent a night
learning, and the four cases are worth restating because they are the reason to
distrust any single figure below that is quoted without its question:

* **`108/108` falsifiable** counted *declarations*. 61 of 108 checks had never
  moved across 64 recordings, and a check with an empty history read the same
  as one that agreed every build.
* **`ci/unported-inventory.py`'s gap counts** rank by *where work has already
  been done*: a refusal marker needs a host line, so the score correlates with
  lines written at r = 0.760, permutation p = 0.0001.
* **`git rev-list --count`** counted commit *objects*. One question, four
  answers: 4,182 naive, 2,587 sha-union, 2,488 patch-id-with-duplicates, 1,377
  distinct changes.
* **`union_N` versus `innocent_N`** mixed two nulls moving in opposite
  directions as N grows. Scored consistently the headline went 14x to 3.53x.

So the three numbers this document is for are stated with their questions
attached, and each has a section defending it.

| | number | the question it answers |
|---|---|---|
| **How complete** | **about 10%**, range 5 to 18% | what share of a vanilla 26.2 dedicated server's behaviour this server reproduces, weighting worldgen as 15 to 20% of the whole |
| **Where it stands** | **leads on one axis of five** | of playable server, worldgen parity level, evidence quality, speed, and community, gritmc leads only evidence quality |
| **Probability it finishes** | **2%, 10%, or 65%** | depending on which of three definitions of "finished" is meant, and they are not close to each other |

A fourth section, of equal weight, says what should change about how the work
runs. It is grounded in the last thirty hours, which were instrumented.

---

## 1. How complete: about 10%

### 1.1 The instrument, and why it is a new one

`ci/unported-inventory.py` cannot answer this. It scans our own tree for
refusal markers, so it can only find debt in files somebody already opened, and
its ranking is a ranking of our own attention. `SpawnEggItem` is the worked
example: an entire entity-shaped gap with no marker anywhere.

`ci/surface-census.py` enumerates the denominator from the **jar's own
generated reports**, which cannot know where our lines are. Its two columns
mean different things and both are needed:

* **named** is every vanilla id whose bare path appears anywhere in 307 `.rs`
  files, in code or in a comment. It is an **upper bound on implementation**
  and a generous one. `world_send` names every block it can serialise a state
  for, and serialising a state is not behaviour.
* **nowhere** is a **lower bound on what is missing**. An id absent from
  157,182 lines of Rust is not implemented, and no argument about instrument
  bias changes that.

Where a stricter numerator exists, it is given beside the named one, and it is
always much smaller.

### 1.2 The census

Measured today against `main` (e2fc07600, 2,324 commits) and vanilla 26.2's
`generated/reports`.

| surface | vanilla | named (upper bound) | implemented (strict) |
|---|---|---|---|
| blocks | 1,196 | 491 | see 1.3 |
| block states | 32,366 | | |
| items | 1,537 | 457 | |
| entity types | 158 | 29 | **2** (`PLAYER`, `ITEM`) |
| of which mobs | 114 | | **0** |
| block entity types | 49 | 42 | **3** (chest, furnace, sign) |
| command roots | 92 | | **2** (`list`, `seed`) |
| serverbound packets | 69 | | **21** can be sent at all |
| clientbound play packets | 141 | 58 | 40 legible, **29** emitted and observed |
| sound events | 1,968 | 34 | |
| status effects | 40 | 11 | |
| particles | 125 | 24 | |
| menu types | 25 | 13 | |
| data components | 111 | 19 | |
| attributes | 40 | 7 | |
| potions | 46 | 7 | |
| villager professions | 15 | 3 | |
| fluids | 5 | 5 | |
| loot tables | 1,355 | | **0 run** |
| recipes | 1,585 | | **1,056** loaded (shaped + shapeless only) |
| advancements | 1,688 | | **0** |
| biomes | 66 | | 66, placed exactly (`biomes=N/0`) |
| structure sets, worldgen dump | 20 | | 8 |
| structures, worldgen dump | 34 | | 21 |
| structure sets, live server | 20 | | **0** |

Two rows deserve their footnote in the table rather than below it. **Recipes
moved**: `crates/rustmc-server/src/recipes.rs` loads
`minecraft:crafting_shaped` and `minecraft:crafting_shapeless` out of
`data/vanilla/recipes.json`, which is 1,056 of the 1,585 entries, and
`docs/SERVER.md`'s line that the file "is read by nothing" is now stale. The
other 529 are stonecutting, smelting, blasting, smoking, campfire cooking,
smithing and the special recipes, and none of those paths exists.
**Structures are two different answers**, and the difference is not a detail:
the worldgen dump places 8 of 20 sets, and the running server places none of
them. The parity figures come from the dump. The world a player would join is
not the world that was measured.

The protocol rows are the repo's own `ci/corpus-surface.py`, run today, and
they are better than the figures this assessment was briefed with. The
clientbound row deserves its detail: of 141 play packets, 58 are named in
`rustmc-proto`, 40 have a decode arm, 38 are actually emitted by vanilla during
a corpus run, and 29 of those are looked at by a check. Nine that vanilla
speaks are heard by nobody, and they need no new scenario because the
recordings already exist.

### 1.3 Where the strict numerator is genuinely hard, it is said so

Blocks have no honest single numerator. The same surface reads 491 named, 39
classes with distinct `useWithoutItem` behaviour, 17 of 184 covered for
`useItemOn`, and 55,077 of 103,482 `(state, direction)` pairs classified for
`update_shape`. Those are four questions. The one that best predicts "would a
player notice" is the third, and it is the worst of the four.

### 1.4 What the gameplay corpus does and does not say

The most recent full corpus run on this box, 2026-08-19 05:48 from
`~/rustmc-stack/oracle-work/gameplay/report.json`, against cold-booted
unmodified vanilla 26.2:

```
declared   97 match / 108   (8 diverged, 3 absent, 0 vacuous, 0 inert)
strict     93 match / 108   (14 diverged, 1 absent, 0 vacuous, 0 inert)
```

That is a large real improvement: on 2026-08-07 the same instrument read
declared 56/86 and strict 35/86.

**It is not a parity percentage and must never be quoted as one.** It answers
"of the 108 scenarios this project wrote, how many produce traces our own
comparator cannot distinguish". The corpus names **66 distinct vanilla ids** in
total. The projection discards or normalises 16 fields before any comparison,
and two of the sixteen decide a live claim. 12 of the 37 packet kinds `decode`
can emit never enter a recording at all. 41 unported paths in
`crates/rustmc-server` are reached by no scenario. And 64 of the checks had
never moved across 66 recordings, so most of the 93 are checks that have not
yet demonstrated they can fail.

86% of a corpus that touches 66 of roughly 2,700 block and item ids is not 86%
of anything a player would recognise.

### 1.5 So: about 10%

The code is 157,182 lines of Rust, and its distribution is the clearest single
statement of where this project actually is:

```
rustmc-worldgen   89,222   57%
rustmc-server     59,481   38%
everything else    8,479    5%   (api, behavior, data, entity, proto, world)
```

Worldgen is the finished part and it is roughly half the code. In a vanilla
server, worldgen is perhaps 15 to 20% of the behaviour. Gameplay, entities, AI,
items, containers, advancements and commands are the rest, and there gritmc is
at 0 to 6% on nearly every row above.

* If worldgen is 20% of a server and gritmc has 70% of it: **16%**.
* If worldgen is 15% and gritmc has 60% of it: **11%**.
* Weighted by vanilla's own item counts, where 1,688 advancements and 1,355
  loot tables dominate: **2 to 4%**.
* Counting "we wrote the id down somewhere" as done: **30 to 35%**.

**Take 10%, and read the range 5 to 18% as the honest width.** The 30 to 35%
reading is the one to be most careful with, because it is the number this
tree's own greps will keep producing.

### 1.6 The velocity, which matters more than the level

The same census run against `main` as it stood on four Tuesdays:

| snapshot | blocks named | items named | entity types named | block entities named | command roots | corpus scenarios | .rs files |
|---|---|---|---|---|---|---|---|
| 2026-07-28 | 205 | 201 | 14 | 14 | 0 | 0 | 213 |
| 2026-08-04 | 246 | 244 | 20 | 21 | 0 | 63 | 255 |
| 2026-08-11 | 397 | 385 | 28 | 32 | 2 | 93 | 298 |
| 2026-08-18 | 490 | 456 | 29 | 42 | 2 | 109 | 305 |

Three weeks of full-tilt agent work, at roughly the whole compute budget per
week, moves the **upper bound** by about 95 blocks and 85 items a week, one
entity type a week, and **zero commands in the last fortnight**.

Extrapolating the upper bound to full coverage: blocks 7 weeks, items 13 weeks,
entity types 26 weeks, command roots **180 weeks**. Those are the optimistic
figures, because they extrapolate *naming*, and naming is where the strict
numerators show 2 of 158 and 2 of 92.

---

## 2. Where it stands: leading on one axis of five

Clean room throughout. Nothing below comes from either project's source. It is
all READMEs, docs sites, blog posts and the GitHub API, pulled 2026-08-19.

### 2.1 The two comparators, objectively

| | gritmc | SteelMC | Pumpkin |
|---|---|---|---|
| public | no | yes | yes |
| stars | n/a | 303 | **10,777** |
| forks | n/a | 43 | 740 |
| contributors | 1 operator plus an agent fleet | 27 | 100+ |
| commits, last 52 weeks | 2,324 in 4 weeks | 733 | 1,114 |
| commits, recent weeks | ~60 to 150 a day | 18 to 27 a week | 48, 71, 66 a week |
| started | 2026-07-21 | 2025-10-15 | 2024-07-28 |
| targets | Minecraft 26.2 | **Minecraft 26.2** | latest Java and Bedrock |
| licence | AGPL-3.0 + CLA intended | AGPL-3.0-or-later | GPL |
| releases | none | v0.15.2, Docker images | yes |

Pumpkin has gained roughly 2,500 stars in the month since `docs/intel/graveyard.md`
was mined (8,220 to 10,777). It is accelerating, not stalling.

SteelMC's own contributor rule is worth reading twice given how this repo is
built: *"AI may be used as a tool, but contributors must understand and be able
to explain every line they submit. Fully autonomous pull requests are not
accepted."*

### 2.2 Axis one, a playable server: behind both, badly

SteelMC ships releases and Docker images and says clients "can join a
persistent multiplayer world, move and interact, use inventories and commands,
and return later to saved chunks", with "early entity, block entity, and
gameplay behavior implementations" and, per their docs, "entities with AI
goals", block entities, container handling and signed chat.

Pumpkin ships Java and Bedrock, inventory, hunger, experience, eating,
liquid physics, world persistence, an advertised plugin API, and marks mobs,
entity AI, combat and redstone work in progress.

gritmc has 2 entity types, 0 mobs, 2 commands, 3 block entity types, no
structures in the running server, no loot tables, no advancements, no argument
nodes in the command tree, and answers an unknown command with silence. A
client can join, walk, dig, place, craft at a table, put things in a chest, and
watch a coral die.

**Both comparators are ahead of gritmc here and it is not close.**

### 2.3 Axis two, worldgen parity level: behind on the overworld, ahead on rigour for the End

SteelMC's claim, from their README and their announcement post of 2026-07-28:
their parity suite "compares 7,500 randomly selected chunks with a reproducible
vanilla reference: 2,500 in each dimension. All tested chunks match block for
block." Their docs put it as "100% vanilla parity checked over 2500 chunks in
all dimensions". Entity spawning is excluded and they say so. They first
reached block-for-block terrain parity on 2026-05-20.

gritmc's comparable figures, from `docs/PARITY.md` §2:

| dimension | best quotable figure | floor beside it |
|---|---|---|
| the End, r=16, 71,368,704 blocks | **0**, byte-identical to one of six unmodified references | 252 (N=6) |
| overworld, seed 12345, r=16, order matched | 69,288 of 107,053,056, **99.935277%** | 2,577 to 4,835 (N=5) |
| nether, r=16 | 254,799 | 317,725, **under floor, no resolving power** |

**The established headline holds, and it is worth less than it looks.**
SteelMC's reference really is a modified vanilla: they wrote a "vanilla
extractor" and changed it to reset cache state per chunk, generate reference
chunks sequentially in a fixed order, and use explicit ordering for hash-based
collections. Their words: "with that final source of nondeterminism removed,
all tested chunks matched block for block."

Three things follow, and only the first is the one this project has been
telling itself.

1. A parity claim against a determinised reference is a different claim from
   one against the jar as shipped. Their number cannot be compared to a floor,
   because they removed the thing a floor measures.
2. **They published the methodology.** It is in the announcement post, in
   plain terms. This is not a concealed modification, and a launch headline
   that implies concealment will not survive a reader clicking the link.
3. **gritmc has the same lever and has never pulled it.** `-Dmax.bg.threads=1`
   makes a vanilla harvest reproducible without modifying vanilla at all, it
   is a JVM flag rather than a patch, `ci/gen-vanilla-region.sh` has stamped
   it in `HARVEST-ORDER` since 2026-08-18, and **every published floor in this
   repo is an unclamped floor**. Nothing has been measured against a clamped
   reference.

So the honest reading of the axis. On the overworld gritmc is behind: 69,288
mismatched blocks against a claim of zero, and order-matching has already
removed the traversal noise, so most of that residual is real. On the End
gritmc has something SteelMC does not, which is zero **against six independently
generated unmodified references**, a strictly harder statement than zero against
one determinised one. On the nether gritmc has no measurement at all, because
vanilla's own two runs disagree by more than our whole residual.

Pumpkin makes no measured worldgen parity claim of any kind. Against Pumpkin,
gritmc is ahead on this axis by default.

### 2.4 Axis three, evidence quality: gritmc leads, clearly, and it is the only axis

Nobody else publishes a floor. Nobody else publishes a denominator. Nobody else
registers `quotable = false` on their own best result the day it was measured,
runs a gate that refuses a check with no recorded red-when-broken evidence, or
keeps a retracted figure in the file labelled rather than deleting it.

SteelMC is the closer of the two and still does not publish per-dimension
residuals, a comparison unit, a statement of what a "chunk match" includes
(block entities? heightmaps? full states or block names?), or any measure of
their reference's own variance. Their claim is stronger in scope and thinner in
method. gritmc's is narrower in scope and far better evidenced.

This is a real lead and it is worth stating plainly what it is worth: it is a
lead in **methodology**, not in product, and methodology does not by itself make
anyone run the server.

### 2.5 Axis four, speed: the pillar is contested and probably lost on wall clock

gritmc's published position, `docs/BENCH.md` and the README: a finished chunk
costs less CPU than vanilla, Paper or Folia (0.082364 CPU-s against 0.189644,
0.114578, 0.130592 at r=7), 0.0607 CPU-s per delivered chunk at r=32, and
**Paper still wins the clock**. The README also records why: rustmc peaks at
1.00 cores while the JVMs use 8 to 14.

SteelMC's published position: "In a focused benchmark on a Ryzen 9 9950X,
SteelMC generated a fresh 10,201-chunk Overworld area in a median of 3.98
seconds", with a reproduction page linked.

That is roughly **2,560 chunks a second**. gritmc's r=7 figure is 225 chunks in
6.55 s wall, roughly **34 a second**, on a 2017 Threadripper 1920X.

**This is a flag, not a verdict, and the caveats are large**: different radii,
different hardware generations, different masks, and gritmc's r=7 square pays
37.7% of its work into a decoration-margin skirt it discards. But the gap is
about 75x on wall and the per-core CPU numbers do not close it, because the
binding difference is that gritmc's generator runs on one core and SteelMC's
does not.

The cheap, clean-room-safe check is to download SteelMC's released binary and
run their published benchmark on the Threadripper beside `worldgen-dump`. That
is running a competitor's shipped artefact, which is ordinary competitive
measurement, and it settles the pillar one way or the other in an afternoon.
**Do it before publishing any speed claim.**

### 2.6 Axis five, community: not in the race

0 stars against 303 and 10,777. One operator against 27 and 100+ contributors.
`docs/intel/graveyard.md`'s own conclusion was that Pumpkin's survival is
community mass and that MCHPRS and PicoLimbo survived by picking one vertical
and finishing it. gritmc currently has neither.

### 2.7 The scoreboard

| axis | gritmc vs SteelMC | gritmc vs Pumpkin |
|---|---|---|
| playable server | **behind** | **behind** |
| worldgen parity level | behind (overworld), ahead (the End), unmeasured (nether) | **ahead** |
| evidence quality | **ahead** | **ahead** |
| speed, wall clock | **probably behind, unverified** | unknown, both make claims |
| community and momentum | **behind** | **far behind** |

---

## 3. The probability it finishes: 2%, 10% or 65%

"Finished" has to be defined before it can be estimated, and the three
plausible definitions have probabilities an order of magnitude apart. Quoting
one number without its definition would be exactly the error section 0 is
about.

### 3.1 The assumptions every figure below rests on

State them first so a reader can reject the conclusion by rejecting an
assumption.

1. **The workforce is one person and an agent fleet.** No second human
   contributor, now or assumed.
2. **The compute ceiling is £40 a week and is already fully spent.** The Max
   20x plan is about £37 a week on its own. `cc-budget` at 17:57 today: 7-day
   **92.0%** with the reset not until Sun 23 Aug 07:00, burn 0.17 %/h against
   0.09 %/h sustainable, roughly **two agent runs left in the week**, and
   credits disabled with no overage cushion.
3. **The +50% weekly boost expires today, 19 Aug.** From tomorrow the same
   work costs 1.5x of a smaller weekly cap. This week's 92% is about **138% of
   a standard week**, so the current shape is running at roughly 1.4x
   sustainable capacity before September.
4. **The owner's available hours fall sharply from September**: Y13, plus an
   AQA CS NEA he must author himself and which Claude may only tutor.
5. **Mojang keeps shipping.** Roughly two majors a year, each resetting
   protocol, data and behaviour. gritmc is pinned to 26.2 in 20 `ci/` scripts,
   and the "48h-SLA automated port pipeline" that `docs/intel/graveyard.md`
   names as the anti-churn survival organ **does not exist in `ci/`**.
   SteelMC's README says it "tracks the latest Java Edition release".

### 3.2 Definition A, something quotable: 65% (range 50 to 80)

*A public artefact, README plus receipts, whose numbers survive a hostile
reader who clicks through to the methodology.*

Most of this already exists. The End at r=16 is 0 against six independently
generated unmodified references with a floor of 252, and that is a genuinely
strong, genuinely novel claim that no competitor is making. The seed 12345
overworld window carries 99.935277% with its floor travelling beside it. The
gate machinery, the falsifiability checks and the retraction discipline are
publishable as they stand.

What blocks it is measurement work rather than porting: **no corpus score has
been quotable since 2026-08-14** because the comparator's projection changed
and every floor predates it, six cold vanilla runs re-take them; the r=16
overworld rows need re-measuring on the current pair; and the speed claim needs
checking against SteelMC's published benchmark before it is printed. That is
days of gate time and writing, not months of ports.

The residual risk is not technical. It is that the owner's time collapses in
September before the receipts are published, and that a project sitting private
with no release is the most common shape on the graveyard list.

### 3.3 Definition B, a server a stranger would run for an evening: 10% (range 5 to 20)

*Join, survive a night, mobs spawn and fight back, craft, use a chest, sleep,
a working command set.*

The remaining scope, from the census rather than from gap counts:

* **114 mob subtypes**, none implemented. One mob tick enters 1,754 methods
  across 240 `ai/**` classes; the entity hierarchy declares 3,870 methods
  across 115 classes; 86 spawning categories.
* **90 more command roots**, and with them roughly 50 `ArgumentTypeInfo`
  serialisers, redirects and suggestions. `Node` has no argument variant at
  all today.
* **46 more block entity types**, **1,355 loot tables** none of which is run,
  **1,688 advancements**, **529 recipes** on paths that do not exist, and
  **20 structure sets** in the live server rather than the dump.

The observed rate against that: **two entity types implemented in four weeks,
one named a week, and zero command roots in the last fortnight**, at roughly
the entire weekly compute budget. Mobs alone, at ten times the observed
implementation rate, is a year of weeks that do not exist after September.

10% is not a statement that the work is impossible. It is a statement about a
budget at its ceiling, a boost expiring today, and a school year starting.

### 3.4 Definition C, this repo's own Definition of Complete: 2% (range 1 to 5)

`docs/WORKLIST.md` sets four items. Their current state:

1. **Order-invariant parity >= 99.99% on every measured window.** Not met. The
   best quotable overworld window is 99.935277%, three of twelve rows in
   `docs/PARITY.md` §2 sit under their floors and are therefore not
   measurements at all, and the nether has no resolving power.
2. **Fastest vanilla-parity server** against vanilla, Paper and Folia. Measured
   and **not met**, Paper wins the clock, and section 2.5 says the race may
   have moved.
3. **A server that runs, a real client joins and plays.** This is definition B.
4. **`ci/t0.sh` green and the suite at its baseline.** Met. 111 test binaries,
   1,830 passing, 2 known red at their declared signature.

Item 3 is a strict prerequisite for item 1 being interesting and it is the 10%
above. Item 2 is contested by a competitor with a published, reproducible
benchmark. Compounding honestly gives low single digits.

### 3.5 Definition D, does it join the graveyard: 30% chance it is still worked on in six months

`docs/intel/graveyard.md`'s own dominant failure mode is *death after the
initial playable state*, and gritmc has not reached the initial playable state.
Its second is *no verification story*, which is the one thing this project has
solved. Its sixth is *solo bus-factor and burnout*.

The countermeasure the file names for the sixth is "swarm plus oracle makes
progress durable and restartable". That countermeasure is load-bearing here and
it now has a price attached: the swarm costs the entire weekly budget, and the
budget shrinks tomorrow.

### 3.6 Throughput, priced

The backlog is not an asset. Measured today across all 292 origin branches with
`git cherry` and `git patch-id --stable`, which is the method `CLAUDE.md`
mandates:

```
3,990   naive sum of + commits
2,532   distinct commit objects
1,400   distinct patch-ids        <- the honest number
```

Note that this is a different figure from the 451 in `CLAUDE.md`, and the
difference is the branch set, not the method: that reading covered the gate
queue, this one covers everything on origin. Both are right about their own
question, which is the whole theme of this document.

The gate is serial and its expected cost per landed branch is
`65.2 + ((1-p)/p) x 3.4`, which at p = 0.79 is **66.1 minutes**. Draining 292
branches is therefore about **322 hours of continuous gate time**, or **187
hours** at the 1.725x batching gain that was measured on the real queue and is
still disabled. That is eight to thirteen days during which the machine does
nothing else, and it costs **zero tokens**, which is the important part: the
gate is calendar-bound, not budget-bound.

Meanwhile the backlog **grew while work landed**, 1,342 to 1,377 in a single
window. Production outruns landing, and unlanded work decays, because every
merge to `main` raises the rebase cost of everything still waiting.

### 3.7 What would change these numbers most, ranked

1. **A second human contributor.** Every survivor on the graveyard list has
   either community mass (Pumpkin, 100+ contributors) or one narrow vertical
   finished (MCHPRS, PicoLimbo). SteelMC has 27 contributors and lands 18 to 27
   commits a week without a compute bill. One contributor who can gate their
   own work is worth more than any scheduling change in section 4, and it is
   the only lever that moves definition B out of single digits.
2. **Narrowing what "complete" means to a vertical with receipts.** The End is
   at 0 residual against six unmodified references. "The only Minecraft server
   that can prove its End generation is byte-identical to vanilla, and here is
   the floor, the denominator and the reproduction script" is shippable today
   and is a claim nobody else on the field can make or quickly copy. That is
   the PicoLimbo lesson, and it converts definition A from 65% to near
   certainty while costing nothing.
3. **Stopping branch creation and draining the backlog.** 1,400 unlanded
   patch-ids is inventory that depreciates. See section 4.
4. **Multi-core generation.** The wall-clock pillar is lost at 1.00 cores and
   `docs/WORKLIST.md` already permits concurrent decoration behind an
   order-invariance proof. This is the one engineering change that could move
   the speed axis rather than the parity axis.
5. **One clamped-reference measurement.** `VANILLA_JVM_OPTS=-Dmax.bg.threads=1`
   on a reference harvest. One flag, one harvest, no port. It turns three
   "under floor, no resolving power" rows into real numbers and makes every
   figure here directly comparable to SteelMC's. It is the cheapest
   high-value measurement available and it has never been taken.
6. **Version churn.** 26.3 resets 32,366 block states, 69 packets, every data
   dump, every golden table and every floor. Without a port pipeline this is a
   project-ending event rather than a sprint, and it is the risk that makes any
   estimate past three months soft.

**What would change them least:** more agents, longer chains, and better
ordering of the gate queue. Ordering provably cannot shrink a serial drain, and
that was measured rather than argued.

---

## 4. What should change about how this project works

Everything here is priced from the last thirty hours, which were instrumented,
rather than from general advice about agents.

### 4.1 Stop: running more lanes than the gate can absorb

**Evidence.** `main` went 2126 to 2324, about 198 commits, across thirty hours
of eight to ten concurrent agents. In the same window the unlanded backlog
**grew**, 1,342 to 1,377, and stands at 1,400 patch-ids across 292 branches
today. The gate is serial at 66.1 expected minutes per landed branch.

**The arithmetic nobody has written down.** A serial gate running flat out
absorbs `1440 / 66.1`, about **22 branches a day**. That is the ceiling on
landed work, it does not move with lane count, and every branch produced above
it is inventory that depreciates as `main` moves underneath it.

So the lane count should be set by **the gate's drain rate**, not by the usage
allowance. Eight to ten lanes was a decision about how much budget was
available; it should have been a decision about how much work could land.

**This is a scheduling problem and a batching problem, and the batching half is
already solved and switched off.** Batching measured **1.725x on the real
queue**, 35.0 hours against 60.4, not the idealised 2.3x. Turning it on is
worth 25 hours of gate time per queue of that size, costs nothing, and is
already measured. Do that before touching lane count.

With batching on, the gate absorbs roughly 38 branches a day, and the honest
lane target is the number of lanes whose combined output is under that.

### 4.2 Stop: hand-rolled waiters

**Evidence.** Agent wakes were the largest single waste of the run. One lane
woke at 615k tokens to report "still queued". Another was woken eight times by
harness prunes at 361k each. Several hundred thousand tokens per empty wake,
many times over, for zero work.

`ci/run-detached.sh` is the fix: one wake per sequence, sentinel written by the
wrapper carrying the exit status so it cannot be absent on a failure the payload
never names, with a seven-case falsifier including `kill -9`.

**It is not on `main`.** It lives on `origin/docs-waiter-t2-blindspot`. The
single cheapest change available to this project is landing one script that
already exists and is already falsified. Land it first, before any further
lane is briefed.

### 4.3 Stop: maintaining the reference cache in its current form

**Evidence.** Enabled for gates, it stored **four entries and hit approximately
never**, because its key hashes the scenario set by path and content and every
branch edits scenarios. The 43.3 to 21.6 minute figure that justified it was
measured on re-gating the *same tree*, which is not the workload.

Either key it per-delta, so a branch that touches three scenarios reuses the
recording for the other 105, or delete it. A cache with a measured hit rate of
approximately zero is machinery that has to be understood by every future
reader for no return. The per-delta version is worth estimating properly first:
if 105 of 108 scenarios are reusable on a typical branch, it is most of the
65.2 minutes.

### 4.4 Stop: treating the commit target as a target

**Evidence.** Several lanes finished early and honestly, at 10 rather than 20,
13 rather than 22, 8 rather than 20. One **refused to port anything at all**
because nothing it wrote could have been verified, which is the correct answer
and the most valuable single output of the night.

The 20 to 25 figure in `CLAUDE.md` was written to fix per-bug spawning, and the
thing that actually fixed it was the **subsystem boundary**, not the number.
The evidence for that is in the same run: the lanes that produced the night's
best work were area-shaped, bastion 70,953 to 2,455, signs, the `use_item_on`
dispatch, the mob-surface refutation. None of them was good because it hit a
count.

Keep `AREA:` and keep the rule that a brief naming one claim number is a bug
rather than an area. Change `EXPECTED COMMITS:` to a **sizing sanity check**
rather than a target: "if you cannot see 20 commits of work in this subsystem,
it is not an area, fold it into the lane that owns that subsystem". And say
explicitly in the brief that finishing early and honestly, or refusing to port
on verifiability grounds, is a success and will be read as one. Otherwise the
number trains that behaviour out, and the refusal is the output this project
most needs to keep.

### 4.5 Start: a findings index, because discovery was duplicated five ways

**Evidence.** The same `figure-provenance` defect was found independently by
**five** lanes. The same three packets were derived from two directions by two
lanes. The sound-arithmetic figure rotted in three places.

Five lanes finding one defect is four lanes wasted on a rediscovery, at
area-lane cost. Nothing made one lane's finding visible to the next, because
findings go into `docs/` prose, and a lane reads the routing table rather than
the corpus of everything anyone learned last night.

The cheapest fix is an append-only one-line-per-finding index,
`docs/FINDINGS.md`: date, one sentence, the commit. Every brief points at it and
every lane appends to it. One line per finding is cheap enough that it happens,
and an append-only file has no merge conflict worth the name. The routing table
already exists to keep the reading tax bounded, so this belongs behind it as
one more action-reached document, not inlined into the brief.

The stronger version, if it proves worth it: the orchestrator injects the last
N findings into each brief at spawn time. That costs tokens per lane, so try
the index first and measure whether rediscovery drops.

### 4.6 Start: a mechanical check for "a right number answering a question nobody asked"

**Evidence.** Roughly six of the run's corrections were this one class:
`108/108` measured declarations, `rev-list --count` measured commit objects,
`union_N` measured a mask's width, `frozen` counted checks with no history.
Every one was arithmetically correct.

There is a cheap mechanical check for a large subclass of it, and the precedent
is the comment-asserts-a-guard checker, which cost 14 seconds and no build.

**Require every registered figure to carry a `unit` and a `question` string,
and refuse a figure whose prose noun disagrees with its registry unit.**
`ci/figures.toml` already carries the entry, the floor and the N. Two more
required string fields, and a doc checker that compares the noun beside the
number in prose to the `unit` on its registry entry, catches:

* `108/108 falsifiable` where `unit = "declarations"` and the prose says
  "checks";
* `1,377 unlanded` where `unit = "patch-ids"` and the prose says "commits";
* a residual quoted beside a floor whose `N` field differs from its own.

That last one is pure arithmetic on fields that already exist and would have
caught the `union_N` case on its own.

It will not catch everything. `frozen` counting checks with no history is a
defect in the *definition*, not in the label, and no string comparison finds
it. The rule that does catch that class is not mechanical: **a null result and
an empty result must be different words in every instrument this project
writes**. Worth stating in `docs/ORACLE.md` as a rule, since it has now
produced two separate defects.

### 4.7 Start: settle the corpus lock, because it is an owner decision with hours attached

**Evidence.** The lock parked up to six recordings at once, with waits over
three hours and **450 agent-minutes** accumulated in one measurement.
Concurrency was then measured at **at most 1.3% slowdown** (95% CI 0.994 to
1.013, N=16 per arm) with integrity **0/8 in every arm**, and the incidents
that founded the lock were shown to have had per-tree ports already.

The exclusivity requirement is therefore unexplained rather than justified.
That is not a decision an agent should take, because being wrong corrupts the
reference every number in the repo is measured against. But it is worth
roughly 450 agent-minutes per occurrence, and it should be decided rather than
inherited.

### 4.8 Keep: the gate, exactly as it is

**Evidence.** The eight-repair stack is the argument for the gate and it is a
strong one. Every one of those eight defects was found **only** by the gate and
was reachable by no local check: three stale claim counts, eight provenances
naming no commit, two corpus evidence rows dropped by a replay, and nine bare
`exit 1` in T0's cheap block, each of which would have silently swallowed a
failure.

A serial 65-minute gate that catches nine swallowed failures is not overhead,
and the correct response to it being the bottleneck is batching and a per-delta
cache, not weakening it.

### 4.9 Keep: the falsification discipline, and treat it as the product

It is the only axis on which this project leads the field, it is the only thing
here a competitor cannot copy in a weekend, and it is the only asset that keeps
its value if the server itself is never finished. `ci/corpus-falsifiable.py`,
`ci/liveness-proofs.py`, the floor columns, the retracted figures kept and
labelled, `quotable = false` registered on the project's own best result the
day it was measured.

If definition B never lands, that is still worth publishing, and it is worth
publishing under its own name rather than as a footnote to a server that is 10%
done.

### 4.10 The summary table

| | change | evidence | worth |
|---|---|---|---|
| stop | more lanes than 22 branches a day | backlog grew 1,342 to 1,377 while 198 commits landed | removes inventory that depreciates |
| stop | hand-rolled waiters | 615k and 361k token empty wakes, repeatedly | hundreds of thousands of tokens a lane |
| stop | the reference cache as keyed | 4 entries, hits approximately never | removes machinery with no return |
| stop | the commit count as a target | lanes finished honestly at 8, 10, 13; one correctly refused | keeps the refusal behaviour |
| start | batching, already measured | 1.725x on the real queue, 35.0 h vs 60.4 h | 25 h of gate time per queue |
| start | `docs/FINDINGS.md`, append-only | one defect found five times | four lanes of rediscovery |
| start | `unit` and `question` on every figure | six corrections of one class in one run | catches the class that keeps recurring |
| start | land `ci/run-detached.sh` on `main` | exists, falsified, stuck on a branch | prerequisite for the waiter fix |
| decide | the corpus lock's exclusivity | at most 1.3% slowdown, integrity 0/8 | 450 agent-minutes an occurrence |
| keep | the gate | eight defects reachable by no local check | the reason the numbers are trustworthy |
| keep | one agent per subsystem | every best result of the night was area-shaped | the fix for the 6x collapse |
| keep | the falsification discipline | the only axis gritmc leads | the only asset that survives a stop |

---

## 5. What this week cost, against what it produced

The week of 16 Aug is the first clean one: the owner stopped his other
projects at that reset, so `cc-budget`'s account figure is this project alone.

**Spent.** 92.0% of the weekly cap by 17:57 on Wednesday, with the reset not
until Sunday 07:00. At about £37 a week for the plan that is roughly **£34 of
the £40 ceiling, in three and a half days**, with credits disabled and no
overage available. About two agent runs of headroom remain, and the +50% boost
that made this week's cap what it was expires today.

**Produced, on `main`.** 243 commits: 30 on Sunday, 59 on Monday, 154 on
Tuesday, and the pause since.

**Produced, on the surface that matters.** Between the 11 Aug and 18 Aug
snapshots: **+93 blocks named, +71 items named, +1 entity type named, +10 block
entity types named, +16 corpus scenarios, and zero command roots.** The
unlanded backlog grew.

So the honest price of a week: **the entire compute budget buys roughly ninety
block ids reaching an upper bound of "named somewhere", one entity type, and no
new commands.** That is the number to hold against section 3's probabilities,
and it is the reason definition B sits at 10% rather than 40%.

It is worth saying what that price is not. It is not evidence the agents are
inefficient at what they were asked to do: 1,830 tests pass, the gate caught
eight defects nothing local could reach, and the nether's worst window improved
by a factor of 29 in one lane. It is evidence that **the unit of work being
bought is not the unit of work that finishes a Minecraft server**.

---

## 6. Reproducing every number in this document

A worktree needs `oracle-work` symlinked, as the housekeeping notes already
require: `ln -s ~/grit/repo/oracle-work oracle-work`. Otherwise pass
`--oracle`.

```sh
# The instrument first. It plants a tree whose answer is known and checks the
# counts move; restoring the bare-path regex takes it red.
ci/surface-census.py --selftest

# The census, and the velocity table.
ci/surface-census.py                                   # against this worktree
ci/surface-census.py --json                            # machine-readable
ci/surface-census.py --list-missing 'entity types'     # the 129 never named

# The velocity snapshots: archive main as it stood, census the archive.
sha=$(git rev-list -1 --before='2026-08-11 23:59' main)
git archive "$sha" | tar -x -C /tmp/snap
ci/surface-census.py --repo /tmp/snap --oracle "$PWD/oracle-work/vanilla-26.2"

# The backlog, by the method CLAUDE.md mandates.
for b in $(git branch -r --format='%(refname:short)' | grep -v HEAD); do
  git cherry origin/main "$b" | awk '$1=="+"{print $2}'
done | sort -u | while read s; do git show "$s" | git patch-id --stable; done \
  | awk '{print $1}' | sort -u | wc -l

# The corpus verdicts.
python3 -c "import json;d=json.load(open('oracle-work/gameplay/report.json'));print(d['declared'],d['strict'])"

# The budget.
cc-budget
```

Competitor figures were pulled 2026-08-19 from the GitHub API
(`/repos/{owner}/{repo}`, `/contributors`, `/stats/participation`), from
`steelmc.dev` and its announcement post of 2026-07-28, and from
`pumpkinmc.org` and the Pumpkin README. No source from either project was read,
and neither is named in any code or test in this tree.

## 7. The one-paragraph version

gritmc is about a tenth of a Minecraft server, and the tenth it has is the
hardest tenth to verify and the least visible to a player: worldgen, measured
more honestly than anyone else in the field measures anything. It is behind
SteelMC on playability and on overworld generation, behind Pumpkin on
everything except evidence, probably behind both on wall-clock speed, and not
in the same race on community. Its real and defensible lead is that it can
prove what it claims, and the End at zero against six unmodified references is
a claim nobody else can make. The probability it becomes a server people run is
about one in ten, the probability it becomes something publishable and true is
about two in three, and the difference between those two numbers is the whole
decision in front of the owner.
