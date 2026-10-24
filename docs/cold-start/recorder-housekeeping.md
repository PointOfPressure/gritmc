# Cold start — branch `recorder-housekeeping`

---

### COLD START — branch `recorder-housekeeping` off `main` (`1d109abcf`), 2026-09-06

**Nothing is dropped at record time any more, the classification is one
declared table with a measured reason per row, and `ci/ambient-falsify.py`
refuses a tree where that has come apart.** Worktree `~/grit/work-rec2`,
remote dir `~/rustmc-rec2`. Not merged — the orchestrator gates.

#### THE DEFECT, AND WHY NO GATE COULD SEE IT

`trace::is_housekeeping` dropped twelve clientbound packet types before any
decoder arm ran. `trace.rs`'s own module docs, one screen above it, state the
rule it broke: **record at full fidelity; be lenient only when comparing.**

**No declared verdict was ever affected, and that is exactly why it survived.**
All 108 checks in `ci/checks/gameplay` carry a non-empty `observe` list, so
`compare::project`'s `kinds` filter had already removed those kinds one line
above. Every corpus number this project has published is unchanged by the
repair. What the drop cost is that the **recording** had no answer to "did the
server send this" — so the defect class here is not a check that cannot fail,
it is a *question that cannot be asked*, and no falsifiability gate in the tree
is shaped to catch that.

Two branches written the night before paid for it in cash. `origin/chunk-streaming`
ported `ChunkTrackingView`, `ChunkLevel`, `PlayerChunkSender` and the ticket
arithmetic, and had to build its 117-position chunk-order golden by capturing a
live vanilla server **outside** the corpus, recording the reason as a killed
null: *"the gameplay corpus cannot observe this area at all"*.

#### WHAT REPLACED IT

`trace::FAMILIES` — fourteen rows, each with

* `class`: `Ambient` (recorded, dropped by `compare::project` unless a
  scenario names the kind in its own `observe` list) or `Graded` (compared like
  anything else);
* `tag`: which **measured** mechanism decided the class, from a closed
  vocabulary;
* `reason`: the prose;
* `arm`: whether `decode` has an arm, which its own unit test keeps true.

Two of the fourteen rows are the `move_entity_heartbeat` and `entity_position`
exclusions that already lived in `compare::project` as hand-written `.filter`
lines. They were **the same decision as `is_housekeeping` by a different
mechanism in a third place**, each with a long and correct commentary, and
neither knew about the twelve. There is now one filter and one table.

#### THE MEASUREMENT — TWO FAMILIES OPEN, ELEVEN STAY SHUT

Two independent vanilla runs over **twenty** scenarios (`5*`, which now
includes `546` and `547`), seed 20260803, superflat, view-distance 4,
Threadripper, tree `6dd091ab9`. "agree" is the ordered sequence of a family's
decoded fields per scenario; "+step" adds the step each observation landed
after, which is what `compare::project` puts into the compared value.

```text
  family                   vanA  vanB  ours  agree  agree+step  emit  decision
  set_chunk_cache_center     21    21    21  20/20       20/20     1  OPEN (Graded)
  forget_level_chunk        231   231   189  20/20       20/20     1  OPEN (Graded)
  chunk_batch_start          33    34    20  19/20       19/20     1  keep
  chunk_batch_finished       33    34    20  19/20       19/20     1  keep
  level_chunk_with_light    231   231   189  16/20       16/20     1  keep (undecoded)
  set_time                  114   117   105   7/20        6/20     3  keep
  keep_alive                  0     2     7  19/20       19/20     1  keep
  update_advancements         1     0     0  19/20       19/20     1  keep
  player_info_update          4     6     0  17/20       17/20     5  keep
  move_entity_heartbeat      28    44    36   6/20        6/20     —  keep
  entity_position            33    30    30  14/20       14/20     1  keep
  light_update                0     0     0      —           —     1  keep (unexercised)
  ping                        0     0     0      —           —     0  keep (no emitter)
  tab_list                    0     0     0      —           —     0  keep (no emitter)
```

**`forget_level_chunk` reads 231 from vanilla and 189 from this server.** That
is the Chebyshev square `world_send::ChunkView` walks against vanilla's padded
disc — 81 chunks against 117 at view distance 4 — and it is the first time any
part of it has been visible inside the corpus. It is the whole reason `547`
exists.

`set_time` and `move_entity_heartbeat` are the two hardest keeps at **6 of 20**
each. The heartbeat number reproduces the 2026-08-04 measurement that put its
`.filter` line in `compare.rs` — the least unstable nine checks at exactly 5 of
11, and vanilla reproducing itself under strict on 0 of 55 — from a different
direction with a different instrument.

The two that open were predicted from the bytecode before they were measured:
`ChunkMap.applyChunkTrackingView` offsets 43-56 gate the centre packet on the
centre CHUNK having changed, and the drop list is `ChunkTrackingView.difference`
over two views that are pure functions of position, so set and order are both
determined and a drop is not queued behind a send.

**THE GUARD FIRED ON ITS AUTHOR, AND THAT IS THE PART WORTH READING.**
`update_advancements` was tagged `unexercised` off a preliminary census that
counted zero, and the real run counts 1 in one vanilla run and 0 in the other.
R2 refused the tree by name. Two rules were tightened as a result: every
disagreement tag (`timer`, `client-driven`, `order-not-self-consistent`) now
requires `measured` as well, because a family neither run emitted has no
sequence to disagree about and satisfied all three vacuously — a rule that
could not go red, sitting inside the rule that exists to make rules go red.

#### HOW IT WAS MEASURED — see `ci/checks/ambient-families.toml`

`ci/ambient-census.sh` records vanilla twice from two fresh world directories
and this server once, then `ci/ambient-falsify.py --sweep` does the arithmetic.
Two vanilla runs is the whole point: one run cannot tell "vanilla always says
this" from "vanilla said this once".

**Two self-consistency columns, and the second is the one that decides.**
`self_consistent` compares the ordered sequence of a family's decoded fields
between the two vanilla runs; `self_consistent_stepped` adds the `step` each
observation landed after — which is what `compare::project` puts into the
compared value. A family whose values reproduce perfectly while arriving after
a different step each run would pass the first and flap in every real check.
Only the stepped column may open a family.

**A family neither run emitted scores `false`, never a vacuous `true`.** Every
empty sequence equals every other one, so silence would otherwise read as the
most stable family in the table — an instrument reporting a clean result
because it had stopped looking.

#### THE `no-emitter` TAG IS A JAR FACT, NOT A CENSUS ONE

A census can only say "zero over the scenarios I ran", which over eighteen
scenarios is the same shape of reasoning as the datapack grep that "proved"
bearding inert. `ci/packet-emitter-sites.py` counts `new` instructions instead,
over 7,445 classes under `net/`:

```text
  ping                     0
  tab_list                 0
  chunk_batch_start        1   its OWN static initialiser
  entity_position          1   its OWN class (a static factory)
  keep_alive               1   ServerCommonPacketListenerImpl
  chunk_batch_finished     1   PlayerChunkSender
  forget_level_chunk       1   PlayerChunkSender
  level_chunk_with_light   1   PlayerChunkSender
  light_update             1   ChunkHolder
  set_chunk_cache_center   1   ChunkMap
  update_advancements      1   PlayerAdvancements
  set_time                 3   ServerClockManager x2, MinecraftServer
  player_info_update       5   PlayerList, ServerGamePacketListenerImpl x2,
                               ServerPlayerGameMode, its own class
```

Two zeroes, and they are exactly the two rows tagged `no-emitter`. A site
inside the packet's own class is a **factory**, not an emission, so `1` proves
nothing on its own; `0` proves nothing under `net/` can build one.

#### THE GATE

`ci/ambient-falsify.py`, in T0 beside `corpus-falsifiable.py`, `--check` and
`--falsify`. Four refusals:

* **R1** a family dropped without a declaration — no `return None` in
  `observe`, and no `.filter`/`.retain`/`.take_while` anywhere in the runner
  that names a kind outside the table;
* **R2** a family dropped for a different reason than declared — every `tag`
  is a predicate over the census row (`TAG_RULES`), so a row tagged
  `no-emitter` whose census counted vanilla emitting it is refused, naming
  both numbers;
* **R3** an opened family that no projection reads;
* **R4** a row with no reason, or a placeholder.

`REQUIRED_FLOOR` 15 -> 16 in the same commit.

#### THE CEILING THAT SPENT THE WRONG HALF OF ITS PREMISE

`ci/vanilla-reach-census.py`'s block-entity ceiling is 19 of 49 types and its
`HOUSEKEEPING_SPENT_BY` entry said it holds because the recorder **drops**
`LEVEL_CHUNK_WITH_LIGHT`. That was never the load-bearing half: the chunk
payload carries every block entity with no `getUpdatePacket` filter, so what
keeps a type illegible is that **no decoder arm reads one out of the payload**.
Under the old guard, recording the packet (ceiling moves by nothing) would have
been refused and writing its arm (ceiling moves by 29 of 49, the whole
registry) would have passed. Exactly backwards. The guard now takes the
undecoded set too and is fired a fourth way.

#### NULLS — KILLED, DO NOT RE-RUN

* **Do not census the existing recordings for these families.** All ~110
  `oracle-work/gameplay/vanilla.json` files on the box were taken by the old
  recorder and hold **nothing** of the twelve. Censusing them censuses the
  recorder, not the servers. This is the same null the 2026-08-12
  `trace-housekeeping-blindness` branch recorded, and it is still true.
* **`is_liveness` is untouched and must stay so.** `Session::settle` asks a
  question about the socket — are bytes still arriving — and that never
  depended on whether the bytes were also written down. Deriving it from
  `Class::Ambient` would silently re-open the window-opens-too-early defect the
  moment a chunk family was reclassified. Said at the function.
* **`LEVEL_CHUNK_WITH_LIGHT` will not get a decoder arm here.** Its
  block-entity tag carries no length, so no arm is possible without an NBT
  reader; and the ceiling above spends its being undecoded. Both halves are in
  its row.
* **A `--falsify` "was main green?" arm that re-runs main's whole T0 is not
  worth building.** The blindness half is demonstrated against
  `ci/corpus-surface.py`'s `trace_sets` — the only thing in the tree that ever
  read the classification — applied to `origin/main`'s own sources with the
  same injection.

#### THE HAZARD THIS BRANCH CREATED AND CLOSED, WORTH ONE LINE

`ci/run-detached.sh` hands you the **wrapper's** pid and runs the payload as
its child under `setsid`. Killing that pid orphans the payload and leaves a
pidfile with a dead pid, so `--status`, the printed waiter and the
already-running refusal all say the run finished while it is still going. Two
`ci/ambient-census.sh` runs then raced for one port and one work directory and
the second's `rm -rf "$WORK"` deleted the first's world. **Stop a detached run
with `kill -TERM -<pid>`** and check `pgrep -f '<name>.body.sh'` before
relaunching. Not repaired here — that file is another lane's.

#### WHAT THE TWO BLOCKED BRANCHES CAN REGISTER NOW

Neither branch is touched here and neither should be rebased onto this one;
what changed is that the recorder can see their subject.

* **`origin/chunk-streaming`** — `546-walking-into-the-next-chunk-moves-the-cache-centre`
  and `547-walking-into-the-next-chunk-forgets-what-is-behind` are shipped
  here as the worked precedent. `547` is the one that matters to that branch:
  `world_send::ChunkView` walks a Chebyshev SQUARE and vanilla's tracked set
  is a Euclidean disc padded by two — 81 chunks against 117 at view distance
  4, not nested in either direction — and `forget_level_chunk` is the packet
  that makes the difference visible. Its 117-position golden can stay where it
  is; it is a stronger instrument. What it gains is a corpus check that goes
  red on `main` and green on itself.
* **`origin/entity-tracker`** — `531-a-block-broken-out-of-view-is-not-broadcast`
  observes `block_update`, which was never classified, so that check was
  always recordable. What it could not do is **say why a run went the way it
  did**: with the chunk stream invisible, a silent observer at step 1 and an
  observer that never received chunk z 7 read identically. `set_chunk_cache_center`
  and `forget_level_chunk` are now nameable, so a scenario can assert the
  observer's view actually excluded the chunk rather than assuming it.

#### IN FLIGHT WHEN THIS WAS WRITTEN — `546` AND `547` ARE UNREGISTERED

**A new gameplay check needs three registrations, not one**, and this is not
written down anywhere a brief would find it:

* `ci/checks/gameplay-falsifiable.toml` — `ci/corpus-falsifiable.py --sweep`;
* `ci/checks/gameplay-grades.toml` — `ci/corpus-history.py` then
  `ci/corpus-grades.py --write`;
* `ci/checks/gameplay-baseline.toml` — the verdict, written by hand, because
  it is the row a human is meant to read.

All three need a **full corpus recording pair taken with this branch's own
recorder**, and a partial sweep is refused by name ("a sweep taken over a
recording that predates a scenario cannot reach tier 2 for it"), so the whole
set serialises behind one `ci/t2-gameplay.sh` run. That run is queued —
`ci/run-detached.sh --status rec2d` — and the box-wide corpus lock was **seven
deep with the oldest waiter at 195 minutes** when it joined.

Until it lands, T0 is red on exactly three lines and each names one of the
three artefacts. Nothing else on this branch is waiting on anything.

**And the sweep must be read before it is committed.** `ci/sweep-guard.py`
(from `origin/dimensions`, copied in untracked as an instrument to avoid an
add/add conflict) compares the committed ledger against the swept one and
refuses a demotion: a tier 2 -> 1 move passes `--check` and is still a check
scoring a claim it no longer reads. It runs as the last stage of `rec2d`. If it
reports a demotion, the two families go back to `Class::Ambient` and THAT is
the finding — the checks keep working either way, because a scenario naming an
ambient kind gets it.

#### THE SINGLE NEXT ITEM

**Re-record the corpus-floor ensemble.** This recorder's vocabulary carries 17
`(kind, field)` pairs the standing 127-pair ensemble never produced (34
old-recorder `vanilla.json` files on the box; 93 pairs in the run), so `ci/corpus-floor-report.py`
prints VOCABULARY DRIFT and declares the floor **not quotable** beside any
score taken here. That is correct — the projection reaches those fields now and
could not reach them there, so the two are different instruments under one
projection id — and it is a warning rather than a gate, which means it will be
quietly ignored until somebody acts on it. What closes it: a re-measured floor
under `ci/corpus-floor.sh --update` whose vocabulary matches this recorder's,
with the drift line gone.

### COLD START — branch `recorder-housekeeping`, landing pass, 2026-09-24

**What was blocking it.** After the rebase onto main 3230 the branch recorded
main's record-time drop of empty chunk packets as `chunk_no_block_entities`
with a PLACEHOLDER row, and T0 was red on `ambient-falsify`, on
`every_row_states_why`, and (behind those, unseen) on `corpus-falsifiable` and
`corpus-grades` for the still-unregistered `546` and `547`.

**What was done.**

* `548-walking-into-the-next-chunk-sends-the-row-ahead` names the kind, so the
  row is `Class::Graded` with a measured reason instead of a tag. Census at
  90f2a3ed4 (`AMBIENT_GLOB='5*'`, 21 scenarios, Threadripper): 253 / 253
  vanilla, 207 ours, 21 of 21 agreeing on values and step, `projected = 44`
  (548's own 22 per run). The corpus run on the same tree reads `548`
  **diverge**: 11 chunks per crossing on vanilla, 9 here. Tier 2 in the
  ledger, killed by `drop-measured`, `retype`, `perturb` and `drop-all`.
* `546` match, `547` diverge, `548` diverge — baseline rows, falsifiable
  ledger (all three tier 2) and grades rows, all from that one run.
* `ci/ambient-falsify.py --sweep` counted `projected` over the whole census
  whenever any scenario file named a kind; over recordings that did not even
  contain `548` it gave 462 and R3 would have passed. Now per naming scenario.
* `ci/corpus-falsifiable.py`'s `inject` duplicated the first row at the
  measured step whatever its kind; with ambient families now recorded, that
  row was often invisible to the projection and seventeen tier-2 checks lost
  `inject` from `killed_by` on the re-sweep. Now it duplicates a row the
  projection sees; the self-test case fails on the old code.
* `update_advancements` keeps `timer`: this census read 0 / 1. Across three
  censuses it has been 1/0, 0/0, 0/1 — two strays in six vanilla runs, both
  in the first scenario (`500`, step 0, ~2 s). The tag is census-dependent
  and a 0/0 re-take will redden it; the reason says so. Not gradeable: the
  body is undecoded and carries criterion `obtained` timestamps.

**Killed as a null.** Main's argument that a chunk's arrival is "which batch
landed inside the window": the batch packets flap, the chunks do not.

**Single next item.** Make `world_send::ChunkView` walk vanilla's padded disc;
`547` and `548` go `match` together, and that closes it.

**Addendum, same day, after the rebase onto main 3611.** `update_advancements`
flipped again (0 / 0 at 3e65f5e87), so on the owner's decision it carries a new
tag, `rare-stray`, whose acceptance is exact: at most one observation per
census, never in both vanilla runs, only at a pinned site
(`500`, step 0), jar emitter present. It goes red if something starts
provoking it. Three main-side guards that read the deleted `is_housekeeping`
(`ci/trace_source.py`, `ci/projection-drops.py`, `ci/never-recorded.py`) now
read `trace::observe` and `trace::FAMILIES`; see FINDINGS for the ambient
filter projection-drops could not see.
