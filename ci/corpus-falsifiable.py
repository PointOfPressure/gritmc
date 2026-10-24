#!/usr/bin/env python3
"""Refuse a gameplay check that cannot go red.

# The class this exists to end

This project keeps rediscovering one defect, one instance at a time, and pays
an agent and a decision for each rediscovery. A partial list from two days:

  * a readiness proof that grepped for `listening on` — which the server
    prints *before* it binds, and keeps running after `Address already in
    use`. A server that never bound read as ready;
  * `0 of 539 stacks opaque`, guaranteed by construction because the step that
    fed the decoder wrote a literal `0` added components, so the decoder's
    opaque arm was unreachable;
  * a `window` counter that coupled zero checks — a server that never
    increments it is invisible;
  * `ACCEPT_TELEPORTATION` decoded into a field read by nothing outside its
    own unit tests, behind a doc comment describing a gate no line
    implemented.

One name covers all of it: **an instrument that measures its own input, or a
verdict whose green cannot be turned red.** Individually each is a bug; as a
class it is this project's central risk, because every parity figure published
rests on checks of this kind not being of this kind.

The question `docs/AGENT-BRIEF.md` has asked for weeks — *what would have had
to happen for this to come back red?* — is a good question that was being
answered by hand, by whichever agent thought to ask it. This mechanises it.

# What it does

For every scenario in a corpus it mutates the **subject** side of a recorded
trace pair and asserts the scenario's outcome moves. A scenario no mutation
can move is a scenario that cannot go red, and is refused by name.

**There are TWO corpora and each has its own ledger and its own pins**
(`CORPORA`, below). `--corpus gameplay` is `ci/checks/gameplay` against
`ci/checks/gameplay-falsifiable.toml` and is the default; `--corpus survival`
is `ci/checks/survival` against `ci/checks/survival-falsifiable.toml`, the
scenarios `ci/t2-gameplay-survival.sh` runs on a survival substrate to reach
the four mechanics creative gates out. `DEAD_FIELDS`, `FIELD_UNIVERSE` and
`MAX_UNASKABLE` are reviewed constants about ONE population of scenarios and
do not transfer: a corpus this file does not name is refused, never given
another's.

    --corpus survival --sweep  oracle-work/gameplay/survival/{vanilla,rustmc}.json

Nothing here starts a server, boots vanilla, or takes the corpus lock. It
re-compares recordings that already exist, with the real comparator, in
seconds — the technique `ci/mute-falsify.py` established. The falsification
half of a demonstration never needs the reference to be re-derived, because
the thing under test is our own instrument, not vanilla.

# Two axes, because the historical defects come in two shapes

**Axis A — scenario falsifiability.** Does *this check* read what the server
said **at the step it is named for**? Seven mutations of the subject trace:

    drop-measured   remove every subject observation at the measured step
    retype          rewrite the observation kind to one no check names
    inject          duplicate the first measured observation
    reorder         reverse the measured step's observations
    perturb:<f>     corrupt field `f` in every measured observation
    speak           put an observation where the step recorded none
    drop-all        remove every subject observation for the scenario

**The mutations are NOT of equal strength and grading them is the whole
design.** `drop-all` moves the outcome of very nearly anything — delete a
scenario's whole output and the verdict goes `absent` — and `speak` is the
same in the other direction. A corpus scored on "some mutation reddened it"
comes back 106/106 and has measured nothing. That figure was the first thing
this script produced, and it was wrong in exactly the way the script exists to
catch. So the battery splits:

    EDIT        drop-measured, retype, inject, reorder, perturb
                — change WHAT the subject said at its measured step
    STRUCTURAL  speak, drop-all
                — change WHETHER it said anything at all

and a scenario is graded

    tier 2   an EDIT mutation reddens it: the verdict is coupled to the
             content of the step the scenario is named for
    tier 1   only a STRUCTURAL mutation reddens it: the check notices traffic
             and notices silence, and nothing in between
    tier 0   nothing reddens it: it cannot go red at all

Tier 0 and tier 1 are both refused. Tier 1 is not a weaker pass — it is the
shape of a check that scores a claim it never reads, which is the class.

**Two promotions to tier 2, both narrow, both named, because without them the
gate refuses correct checks.**

*A check that declares its measured step silent* has "nothing here" as its
whole claim, so `speak` — *would you have noticed speech?* — is the only
question that can falsify it, and it counts as strong for that shape alone.
Four checks in this corpus are of it. This is the mechanised form of what
`ci/mute-falsify.py` demonstrates by hand.

*A check that is already red at the step it names* has had its falsifiability
demonstrated by the corpus, which beats any mutation. `604-container-click-
swap` is the case: the feature is not implemented, so the subject contributes
**zero** projected observations at its measured step, so there is nothing
there to drop or perturb and every EDIT mutation is inapplicable. Refusing it
would be refusing a check for correctly reporting a gap, and would block the
branch closing the gap. See `discriminates_at_measured_step` for how narrow
the route is.

**What axis A does not catch, said plainly.** Within this comparator a
scenario that names an `observe` list and appears in the recording is very
hard to make unfalsifiable, because the comparator's job is to notice changed
subject output. So axis A finds no live defect in the corpus today — it scores
the corpus and then holds it there. Its value is the ratchet and the fact that
every future check must earn a tier-2 entry before it can land, not a haul on
the day it shipped. **The live findings on this branch are on axis B.** The
one historical defect that lives in a readiness predicate rather than in a
scenario — `listening on` — is out of this script's reach by construction and
is `ci/liveness-proofs.py`'s subject.

**Axis B — field liveness.** Is *any* check reading this field? Every
`(kind, field)` in the recordings is perturbed and the whole report re-read. A
field nothing reads is the `ACCEPT_TELEPORTATION` shape and the
`window`-counter shape: recorded, decoded, described in a doc comment, and
coupled to no verdict anywhere. These are ratcheted rather than refused
outright — the corpus has a standing population of them and a gate that
refuses the world as it is on the day it lands is a gate that gets deleted.

**Three corrections got axis B to a number worth quoting, and each was this
script committing the defect it exists to refuse.**

*One perturbation strategy is not a detector.* Corrupting **every** matching
observation is a uniform additive offset for a session counter, precisely the
quantity `rebase_session_counters` exists to discard — correctly, since the
harness hands both servers the same number and a uniform offset can only
encode run history — so `block_changed_ack.seq` and the container `window`
cancelled inside the comparator and read as dead. Corrupting only the **last**
changes a *gap* instead of a base and survives the rebase, but is blind where
the other is not: a field consumed by an aggregate built before any filter
runs — the entity position table `project` accumulates — is unmoved by one
row. Measured on the 106-scenario corpus: **11 dead under `all`, 32 under
`last`, and the two lists are not nested.** Both are run and only fields
surviving both are candidates.

*The change detector has to see a changed projection, not just a changed
verdict.* `signature` originally read verdicts, counts and kind sets. A
scenario already at `diverge` stays at `diverge` under almost any
perturbation, with its counts unchanged — so `entity_position.at` was reported
dead when a direct perturbation of all 32 of its observations demonstrably
moves `a-player-that-leaves-the-ground-is-resynced` and nothing else.
`first_difference` is in the signature for that.

*And where a change still might not show, the honest answer is neither.* A
projection change always flips a scenario off `match`, so a field carried only
by matching scenarios is **detectable** and a null result means dead. A field
carried by a scenario that already diverges is not, and is reported
`undetermined` — recorded in the ledger, kept out of `dead`, and deliberately
**not** ratcheted, because it rises when the server regresses somewhere
unrelated and a gate that reddens for that is a gate agents learn to bypass.

# Why the evidence is a committed ledger and not a live sweep

`ci/t0.sh` has no recordings and must not need any: it is the fast gate and it
runs where no server has ever been started. So the sweep writes its findings
to the corpus's ledger and T0 reads *that*, once per corpus:

    --sweep    needs recordings; produces the ledger; seconds; no lock
    --check    needs nothing; refuses on the ledger; this is what T0 runs
    --selftest needs nothing; the house pattern for a T0 python script

`--check` refuses on:

    UNFALSIFIABLE  a scenario the sweep could not move
    UNREGISTERED   a scenario file with no ledger entry at all
    STALE          a scenario edited since its evidence was taken
    ORPHAN         a ledger entry whose scenario file is gone
    RATCHET        fewer falsifiable scenarios than the pinned floor
    BATTERY        evidence taken by a different set of mutations
    UNSWEPT        a scenario the recording did not cover, beyond the
                   pinned allowance. Such a scenario gets NO ledger row at
                   all — filing one at tier 0 would be a positive claim that
                   no mutation can redden it, evidenced by never having run
                   one, and "not measured" is the reading this file exists to
                   keep apart from "measured and found unfalsifiable"

**STALE is the load-bearing one and the reason the digest is semantic.** A
check whose `observe` list or steps changed is a different check, and its old
red-when-broken evidence belongs to the old one; but `about` is prose and a
typo fix there must not invalidate a measurement. So the digest is over the
parsed document with `about` removed, canonically serialised — not over the
file's bytes.

**UNSWEPT is a debt, pinned twice.** A scenario the recording did not cover
has no evidence, and the honest verdict on no evidence is refusal. But the
recordings on the box lag the corpus by a scenario or two at any moment, and a
gate that cannot land is not a gate. So the unswept set is listed by name in
the ledger *and* bounded by `MAX_UNSWEPT` **in this file** — growing the
allowance is a code change with a reviewer, not a data change an agent can
make while adding the check the allowance would hide.

# What this is not

It is not a lint for a naming convention, and it does not read source. It
takes the real comparator, real recordings and real scenario files, breaks the
subject, and reports which checks noticed. A check that notices nothing is
reported by name whatever it is called.

Its own red-when-broken evidence is `ci/corpus-falsifiable-falsify.sh`, which
reconstructs two of the historical defects above and shows this refusing them.
A guard that cannot be shown catching a real past instance is itself a check
that cannot fail.
"""

from __future__ import annotations

import copy
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BIN = ROOT / "target" / "release" / "oracle-runner"
DEFAULT_CHECKS = ROOT / "ci" / "checks" / "gameplay"
DEFAULT_LEDGER = ROOT / "ci" / "checks" / "gameplay-falsifiable.toml"

# The mutation battery, by name. Recorded into the ledger and compared back on
# `--check`: evidence taken by a different set of mutations is evidence about a
# different instrument, and reads as green while meaning nothing.
BATTERY = ("drop-measured", "retype", "inject", "reorder", "perturb", "speak", "drop-all")

# **EDIT mutations change what the subject SAID at the step the scenario is
# named for.** A kill by one of these is the strong evidence — the check's
# verdict is coupled to the content of its own measured step.
EDIT = ("drop-measured", "retype", "inject", "reorder", "perturb")

# **STRUCTURAL mutations change whether the subject said ANYTHING.** A kill by
# one of these alone is weak: `drop-all` deletes a scenario's whole output and
# `speak` inserts an observation where the projection saw none, and a
# comparator that failed to notice either would be broken outright rather than
# subtly. They prove the harness is wired up, not that this check discriminates.
#
# `speak` is promoted to strong evidence for exactly one shape — a check that
# DECLARES its measured step silent. There the claim *is* "nothing here", and
# "would you notice speech?" is the only question that can falsify it. That is
# the mechanised form of what `ci/mute-falsify.py` demonstrates by hand.
STRUCTURAL = ("speak", "drop-all")

# How many scenarios may sit unswept — see the module docstring. This is in
# code, deliberately, so that widening it is a reviewed change rather than a
# line an agent adds to a data file in the same commit as the check it hides.
MAX_UNSWEPT = 4

# A kind no scenario's `observe` list may name, used by the `retype` mutation.
FOREIGN_KIND = "__corpus_falsifiable_mutant__"

# **The recorded fields no check reads, by name, and the size of the
# population they are drawn from.**
#
# These were in the ledger and nowhere else, and that made the one ratchet
# over them incapable of firing. `--check` read `dead_fields` out of `[meta]`
# and compared it against `len(dead_field)` from the same file, written by the
# same sweep run — two numbers that are equal by construction. The refusal it
# was supposed to raise, "a field was added that couples to no verdict", could
# only ever have been raised by hand-editing the file it was checking, which
# the ledger's own header forbids.
#
# That is this script's own defect class, in this script: a verdict whose green
# cannot be turned red. The `floor` and `weak_floor` ratchets are not of it —
# both recount from the scenario *files*, so a check added without a re-sweep
# moves one side and not the other — and the difference is that a dead field
# has no counterpart outside the ledger to recount against.
#
# So the counterpart is here, which is the same answer `MAX_UNSWEPT` already
# gives one line up for the same reason: widening it is a code change with a
# reviewer, not a data line an agent adds in the same commit as the field it
# would hide. A sweep that finds a different dead set now refuses until
# somebody says why the set changed.
#
# `FIELD_UNIVERSE` is pinned for a second reason on top of that: it was written
# into the ledger by the sweep and read by NOTHING — not by `--check`, not by
# any script in this tree. `docs/cold-start/worklist-merge-driver.md` quotes it
# as the denominator of a published ratio, and quotes it as 84 against a ledger
# that says 90. A figure nothing checks is a figure that drifts.
#
# **2026-09-05, `mob-in-world`: the pinned set is EMPTY, and that is not the
# good news it reads as.** The four pairs that stood here —
# `move_entity_rot.entity`, `move_entity_rot.rest`, `rotate_head.entity`,
# `rotate_head.rest` — did not become fields a check reads. They moved into
# `undetermined_field`. A pig and a bat now stand in every recording, so every
# scenario carrying `move_entity_rot` or `rotate_head` diverges on its own
# account, and perturbing those four fields can no longer be *shown* to have
# moved anything. One sweep, one recording pair: dead 4 -> 0 and undetermined
# 3 -> 10, and the seven that moved are those four plus the three below.
# Read `()` as "the sweep can no longer determine these", never as "they are
# read". `undetermined_fields` is deliberately unratcheted (see `check`), so
# no number in this file records that loss and this comment is the record.
#
# **() -> two, 2026-10-01, `be-furnace-brewing` — and it is main's pair that
# says so, not the branch.** `move_entity_rot.entity` and `.rest` left
# `undetermined_field` and came back dead: perturbing them is determinable
# again and moves nothing. Swept twice on the Threadripper over a copy of
# `~/rustmc-stack/oracle-work/gameplay`'s pair as it stood at 16:33 that day
# (md5 vanilla 80a88bf5, rustmc 4fdf7a68, 179 scenarios, main 3a74284a5):
# once alone with main's own `ci/checks/gameplay`, once with this branch's
# subset recordings spliced in. Both write dead 2, undetermined 11, universe
# 137, unaskable 15 — identical — so none of it is the branch's. The likeliest
# mover is 084's pig going silent on main (0ae84efee), which took away the
# divergence that made these two undeterminable; that is a reading, not a
# measurement.
# **two -> (), 2026-10-01, `train-g`.** `move_entity_rot.entity` and `.rest` are
# read again: on the train's splice no (kind, field) pair is dead, 0 of 198.
# Swept on the Threadripper over a splice of `~/rustmc-stack/oracle-work/gameplay`'s pair as it stood at
# 22:32 on 2026-10-01 (md5 vanilla 7af8dbda, rustmc aa7553f9, 212 scenarios, main
# `16529d006`) with the nine branches' subset recordings swapped in for the 148
# checks it lacks (360 scenarios, md5 a5783150 / cd238cd3; `traing/splice2.py`),
# oracle-runner built from this tree: 6205 comparator runs, 360/360 tier 2.
DEAD_FIELDS: tuple = ()

# **90 -> 95, 2026-09-05.** This server started sending `set_entity_motion`
# and `update_attributes`, so five (kind, field) pairs the reference had
# always recorded became askable: `set_entity_motion.{entity,rest,v}` and
# `update_attributes.{entity,rest}`. `unaskable_fields` fell 12 -> 7 by the
# same five, and `reference_field_universe` is unchanged at 102 — the
# reference's vocabulary did not move, ours grew into it. Measured
# `ci/corpus-falsifiable.py --sweep` over the `mob-in-world` recording pair,
# 1759 comparator runs, Threadripper.
# **95 -> 102, 2026-09-24, `mob-goal-corpus`.** The sweep over this lane's
# recording pair (`~/rustmc-mobgoal`, 160 scenarios on the 3611 corpus plus
# `083`-`089`, whose `level_event` observations of the bat's 1025 take-off
# are the first in the corpus about a mob) reads 102 askable pairs and
# `reference_field_universe` 110; `unaskable_fields` 8, under
# `MAX_UNASKABLE`. `T0 RED ... FIELD-UNIVERSE ledger says 102 recorded (kind,
# field) pairs, this file pins 95` on that tree is what this answers. The
# seven are not attributed pair by pair here: the 3611 corpus grew 120 -> 153
# scenarios in the same window.
#
# **90 -> 108, 2026-09-24, `recorder-housekeeping`.** The recorder stopped
# dropping twelve clientbound families at record time, so the subject trace now
# carries pairs it never could: `chunk_batch_start`, `chunk_batch_finished`,
# `chunk_no_block_entities`, `forget_level_chunk`, `set_chunk_cache_center`,
# `keep_alive`, `set_time` and friends. No check's field went dead in the move
# (`DEAD_FIELDS` is unchanged); the denominator grew because the vocabulary
# did. Swept on 90f2a3ed4's own recording pair, 114 scenarios, Threadripper.
# **108 -> 115** on the rebase onto main 3611 (stack-gameplay, 154 scenarios),
# re-swept on 3e65f5e87's own pair: main's new scenarios brought their kinds.
# **115 -> 129, 2026-09-30, `train-a`** (main 3745 + redstone, world-systems-2,
# dimensions, mob-in-world, mob-goal-corpus, block-entity-behaviour-2,
# recorder-housekeeping). Swept on the train's own recording pair, 179
# scenarios, 3033 comparator runs, `~/rustmc-train-a`, Threadripper: 129
# askable pairs, `reference_field_universe` 142, `unaskable_fields` 13, dead 0,
# undetermined 13. The fourteen are the union of the lanes' vocabularies
# (border and scoreboard packets, the mob kinds, the chunk-view families)
# landing on one tree; no single lane's sweep could see all of them.
# **129 -> 137, 2026-10-01, `be-furnace-brewing`.** Main's own pair, swept
# alone (see `DEAD_FIELDS` for the pair and the two sweeps), reads 137; the
# branch's splice reads the same 137, so the eight are main's recorder and
# scenarios, and this branch's checks bring no pair of their own.
# **137 -> 138, 2026-10-01, `item-components`**, on its splice of that same
# pair with 930-952 recorded in (202 scenarios, 3640 comparator runs): the
# one new pair is `container_set_content.truncated`, which a content packet
# carries only once a stack in it has a component patch the recorder cannot
# read past — first in 934-939 and 940-941. Dead, undetermined and unaskable
# are main's exactly.
# **138 -> 198, 2026-10-01, `train-g`** (main 16529d006 + item-components,
# train-e, land-batch-1, menus-workstations, fluids, mining, commands-exec,
# random-ticks, train-f). Swept on the Threadripper over a splice of `~/rustmc-stack/oracle-work/gameplay`'s pair as it stood at
# 22:32 on 2026-10-01 (md5 vanilla 7af8dbda, rustmc aa7553f9, 212 scenarios, main
# `16529d006`) with the nine branches' subset recordings swapped in for the 148
# checks it lacks (360 scenarios, md5 a5783150 / cd238cd3; `traing/splice2.py`),
# oracle-runner built from this tree: 6205 comparator runs, 360/360 tier 2: 198 askable pairs,
# `reference_field_universe` 207, `unaskable_fields` 9, dead 0, undetermined 13.
# The sixty are the nine branches' vocabularies (the workstation menus'
# data slots and buttons, the fluids' and mining's block traffic, the
# animals', orbs' and pickups' entities, the commands' replies, train-e's
# combat and projectiles) on one tree for the first time.
FIELD_UNIVERSE = 198

# **The fields axis B cannot ask about, because our server never sends them.**
#
# Axis B enumerates `(kind, field)` pairs from the SUBJECT trace and perturbs
# each one. A pair the *reference* records and the subject does not has nothing
# to perturb, so the sweep never reaches it — and the sweep reported a
# denominator of 90 while the recorded vocabulary across the pair is 102.
#
# The twelve are not a random tail. They are `set_entity_motion`,
# `set_equipment`, `take_item_entity` and `update_attributes`: the packets this
# server does not send yet, which is exactly where a check claiming coverage is
# most likely to be claiming it of nothing.
#
# "No check reads this" and "nothing exists for a check to read" are different
# findings and were being reported as one number. This is the second, by name,
# and it is ratcheted upward only: a new scenario that makes vanilla emit a
# kind we do not implement is a coverage gap worth a reviewer, and one that
# closes is good news that must not redden a gate.
#
# **12 -> 14, 2026-09-24, `recorder-housekeeping` rebased onto main 3611, and
# no check was added over any of the new ones.** Re-swept on 3e65f5e87's own
# pair, 154 scenarios. In: `player_info_update` (decode, rest) and
# `update_advancements` (decode, rest), which vanilla sends and this server does
# not, visible because the recorder no longer drops them at record time — both
# `Class::Ambient` in `trace::FAMILIES`, named by no scenario — and
# `packet:0x45` (rest), an undecoded id vanilla sends in main's
# `acknowledging-a-teleport-twice-is-refused`. Out: `set_entity_motion`
# (entity, rest, v), which this server now sends. The ceiling rises because
# the instrument got better eyes, not because coverage was claimed of nothing.
# **14 -> 15, 2026-10-01, `be-furnace-brewing`, and no check was added over
# it.** In: `waypoint.chunk_x` and `waypoint.chunk_z`, out: none, so the
# ledger's count goes 13 -> 15 against a ceiling of 14. Main's recorder now decodes `waypoint`, and vanilla
# sends the chunk-anchored form, which this server never does. Main's pair
# alone reproduces it (see `DEAD_FIELDS`).
# **15 -> 9, 2026-10-01, `train-g`, a tightening.** On the train's splice (see
# `FIELD_UNIVERSE`) the reference records 207 pairs and this server never sends
# nine: `packet:0x2F.rest`, `packet:0x45.rest`, `set_equipment.{entity,rest,
# slots}`, `update_advancements.{decode,rest}`, `waypoint.{chunk_x,chunk_z}`.
# `player_info_update` and `set_entity_motion`'s pairs left the list (train-e
# sends them); the ceiling follows the measurement down so the next pair that
# joins is a reviewer's question.
MAX_UNASKABLE = 9

# --------------------------------------------------------------------------
# THE CORPORA — 2026-09-05
#
# `ci/checks/survival` had NO ledger and no T0 hook. This script swept
# `ci/checks/gameplay` and nothing read a survival ledger, so none of the
# survival checks had ever been shown able to go red — and `ci/t2-gameplay-
# survival.sh` exists precisely to grade the four mechanics creative cannot
# reach (damage, health, hunger, mining progress, block drops). A second
# corpus with no falsifiability gate is a second corpus whose greens mean
# nothing, and `origin/player-systems` could wire nothing to the tick loop
# because nothing there could be graded.
#
# The pins below are the reason this is a table rather than a `--checks` flag
# with the gameplay constants behind it. `DEAD_FIELDS`, `FIELD_UNIVERSE` and
# `MAX_UNASKABLE` are reviewed constants about ONE corpus: a survival ledger
# checked against gameplay's 90-pair universe and gameplay's four dead fields
# would be red on every run for reasons that say nothing about survival, and
# an agent's only route out of that is to widen the pins for both. So each
# corpus carries its own, by name, and a `--ledger` this table does not know
# is REFUSED rather than silently given gameplay's — a third corpus borrowing
# a second one's denominator is exactly the shape of defect this file is for.
# --------------------------------------------------------------------------


class Corpus:
    def __init__(self, key: str, checks: Path, ledger: Path, title: str,
                 dead_fields: tuple, field_universe: int,
                 max_unaskable: int, max_unswept: int) -> None:
        self.key = key
        self.checks = checks
        self.ledger = ledger
        self.title = title
        self.dead_fields = dead_fields
        self.field_universe = field_universe
        self.max_unaskable = max_unaskable
        self.max_unswept = max_unswept


GAMEPLAY = Corpus(
    "gameplay", DEFAULT_CHECKS, DEFAULT_LEDGER,
    "gameplay-falsifiable.toml — which gameplay checks have been SHOWN to go red.",
    DEAD_FIELDS, FIELD_UNIVERSE, MAX_UNASKABLE, MAX_UNSWEPT)

# **The survival pins are SMALLER and that is the whole point of separating
# them.** Five scenarios on a survival substrate record a much narrower
# vocabulary than 108 creative ones, and `damage_event` / `set_health` /
# `update_attributes` are *unaskable* in the creative corpus (this server does
# not send them there) while being the entire reason this one exists. Filled
# in from the first sweep's own printout on 2026-09-05 and reviewed here, in
# code, exactly as the gameplay ones are.
# Measured 2026-09-05 on the Threadripper, `~/rustmc-corpus` at main tip
# bd5be4a3 + this branch's ci-only changes: `ci/t2-gameplay-survival.sh`
# recorded all four scenarios on both sides (vanilla 26.2 on :25898,
# gamemode=survival, natural_health_regeneration false), then
# `--sweep --corpus survival`, 71 comparator runs. The sweep's own printout:
#
#   axis A  tier 2 (its own step reddens it)    4/4 swept (0 not in recording)
#   axis A  tier 1                              0
#   axis A  tier 0                              0
#   axis A  no localised kill                   0
#   axis B  dead fields   0/15 (kind, field) pairs no check reads
#   axis B  undetermined 10
#   axis B  unaskable    41 of 56
#
# **ZERO dead fields, against gameplay's four.** Not a virtue of these
# scenarios; a consequence of the corpus being four checks over a 15-pair
# vocabulary, where a field nothing reads has almost nowhere to hide. The
# number to watch is the other one.
#
# **41 unaskable of 56 is the honest headline of this whole pass.** Three
# quarters of what vanilla says on a survival substrate — the whole of
# `damage_event`, `set_health`, `block_destruction`, `add_entity`,
# `set_entity_motion`, `sound`, `level_event` — this server never sends, so
# axis B has nothing to perturb and no check can be asked whether it reads
# them. That is a coverage statement about the SERVER, not about the checks,
# and it is exactly the population `ci/t2-gameplay-survival.sh` was built to
# make visible: those packets sit below early returns that creative could
# never get past. It is ratcheted upward only, so the number falling is this
# server implementing something and must not redden a gate; the number rising
# is a new scenario reaching a packet nothing here sends.
SURVIVAL_DEAD_FIELDS: tuple = ()
# **15 -> 20 and 41 -> 38, 2026-09-23, `player-systems-2`.** Scenarios 005
# (hurt cooldown) and 006 (five-block fall) added two recorded kinds' worth of
# fields, and this server now sends `set_health`, so its four fields left the
# unaskable list. Swept over `~/rustmc-ps2`'s recording pair of 2026-09-06
# 04:18 (6 scenarios, vanilla 26.2 survival on :25898), oracle-runner built
# from this branch on main 3316: 6/6 tier 2, 0 dead of 20, 38 unaskable of 58.
# **20 -> 71 and 38 -> 34, 2026-09-30, `damage`.** Ten `/damage` checks
# (007..016, and 006 widened) record `damage_event`, `set_entity_motion`,
# `set_entity_data`, `sound`, `entity_event` and `chat` on both sides, and this
# server now sends all of them for a hurt player, so four unaskable pairs left
# the list while 51 new recorded pairs joined the universe. Swept over
# `~/rustmc-damage`'s pair of 2026-09-30 16:21 (15 scenarios, vanilla 26.2
# survival), oracle-runner built at `6328a25cc`: 15/15 tier 2, 0 dead of 71,
# 34 unaskable of 105.
# **71 -> 108 and 34 -> 20, 2026-10-01, `train-g`.** mining's survival checks
# (040-055: digging, tools, Silk Touch, drops) join damage's 005-016 and main's
# four, and the breaking traffic they record — `block_destruction`, `level_event`,
# the dropped item's `add_entity`/`set_entity_data`, the worn tool's slot — is
# traffic this server now sends, so fourteen unaskable pairs left the list and 37
# askable ones joined. Swept over a splice of the newest recording of each of the
# 30 survival checks (`~/rustmc-mining-t0/oracle-work/gameplay/survival`, 19, and
# `~/rustmc-entpvp/oracle-work/gameplay/survival`, 11), oracle-runner built from
# this tree: 776 comparator runs, 30/30 tier 2, 0 dead of 108, 20 unaskable of
# 128.
SURVIVAL_FIELD_UNIVERSE = 108
SURVIVAL_MAX_UNASKABLE = 20

# **The unswept allowance is 0 here, not 4.** `MAX_UNSWEPT = 4` is a tolerance
# sized against a corpus of 108: four scenarios awaiting a re-record is a
# rounding error there. Against FOUR survival scenarios the same number is an
# allowance to leave the ENTIRE corpus unswept and pass — a ceiling above its
# own population is not a ceiling. Nothing needs it: the survival pass records
# all four in one run, and a fifth scenario arriving without a recording has an
# honest answer, which is to record it.
SURVIVAL_MAX_UNSWEPT = 0

SURVIVAL = Corpus(
    "survival", ROOT / "ci" / "checks" / "survival",
    ROOT / "ci" / "checks" / "survival-falsifiable.toml",
    "survival-falsifiable.toml — which SURVIVAL checks have been SHOWN to go red.",
    SURVIVAL_DEAD_FIELDS, SURVIVAL_FIELD_UNIVERSE, SURVIVAL_MAX_UNASKABLE,
    SURVIVAL_MAX_UNSWEPT)

CORPORA = {c.key: c for c in (GAMEPLAY, SURVIVAL)}
BY_LEDGER = {c.ledger.name: c for c in CORPORA.values()}
BY_CHECKS = {c.checks.resolve(): c for c in CORPORA.values()}


def corpus_for(checks: Path, ledger: "Path | None" = None) -> Corpus:
    """The reviewed pins for this corpus, or a refusal.

    **Keyed on the SCENARIO DIRECTORY, not on the ledger's filename.** The pins
    describe a population of scenarios — which `(kind, field)` pairs their
    recordings contain, and which of those no check reads — so they follow the
    checks directory wherever the ledger happens to be written. That is what
    lets `ci/corpus-machinery-falsify.py` build a scratch ledger at an
    arbitrary path over the REAL corpus and still be graded against the real
    pins, which is the whole point of that suite; keying on the filename broke
    six of its cases with rc=2.

    Never a default. A scenario directory this table does not name would
    otherwise be graded against some other corpus's dead-field set and some
    other corpus's denominator, and pass.
    """
    c = BY_CHECKS.get(checks.resolve())
    if c is None and ledger is not None:
        c = BY_LEDGER.get(ledger.name)
    if c is None:
        die(f"no pinned corpus for scenario directory {checks} — add one to "
            f"CORPORA in ci/corpus-falsifiable.py.\n         Known: "
            f"{', '.join(sorted(CORPORA))}.\n         The dead-field set and the "
            f"field universe are reviewed constants about ONE population of\n"
            f"         scenarios; a new corpus may not borrow another's.")
    return c


def die(msg: str, code: int = 2) -> None:
    print(f"CORPUS-FALSIFIABLE REFUSED — {msg}", file=sys.stderr)
    sys.exit(code)


# --------------------------------------------------------------------------
# scenario files
# --------------------------------------------------------------------------


def semantic_digest(doc: dict) -> str:
    """sha256 of a scenario's meaning, with its prose removed.

    `about` is documentation; every other key changes what is measured. See
    the module docstring on STALE for why this is not a digest of the bytes.
    """
    body = {k: v for k, v in doc.items() if k != "about"}
    canon = json.dumps(body, sort_keys=True, separators=(",", ":"))
    return "sha256:" + hashlib.sha256(canon.encode()).hexdigest()[:32]


def load_checks(checks: Path) -> dict[str, tuple[Path, dict]]:
    """name -> (file, parsed document), refusing a corpus with two of a name."""
    out: dict[str, tuple[Path, dict]] = {}
    for p in sorted(checks.glob("*.toml")):
        try:
            doc = tomllib.loads(p.read_text())
        except tomllib.TOMLDecodeError as e:
            die(f"{p.name} does not parse as TOML: {e}")
        name = doc.get("name")
        if not name:
            die(f"{p.name} declares no name")
        if name in out:
            die(f"two scenario files declare name {name!r}: {out[name][0].name}, {p.name}")
        out[name] = (p, doc)
    if not out:
        die(f"no scenarios in {checks}")
    return out


# --------------------------------------------------------------------------
# the comparator
# --------------------------------------------------------------------------


def run_compare(binary: Path, checks: Path, ref: Path, sub: Path, out: Path) -> list[dict]:
    # **The report path is reused for every run, so a stale one must not be
    # read as this run's.** `sweep` writes thousands of reports to the same
    # file; without this, a comparator that died before writing would hand the
    # PREVIOUS mutation's outcomes back as its own — a red read as whatever the
    # last run said, the exact class this file exists to refuse.
    out.unlink(missing_ok=True)
    r = subprocess.run(
        [str(binary), "compare", str(checks),
         "--reference", str(ref), "--subject", str(sub), "--json", str(out)],
        capture_output=True, text=True,
    )
    # A non-zero exit is ordinary here: a mutated corpus is a red corpus, which
    # is the point. Only a missing report is fatal.
    if not out.exists():
        die(f"compare wrote no report (rc={r.returncode})\n{r.stdout}\n{r.stderr}")
    return json.loads(out.read_text())["outcomes"]


def signature(outcomes: list[dict]) -> dict[str, tuple]:
    """Per scenario, everything the report says except its prose.

    Richer than the verdict alone on purpose, and `first_difference` is in
    here because leaving it out cost a wrong number. **A scenario that is
    already `diverge` stays `diverge` under almost any perturbation, and its
    observation counts and kind sets can be unchanged too** — so a field read
    only by diverging scenarios moved nothing in this tuple and was reported
    dead. `entity_position.at` was: it is read by
    `a-player-that-leaves-the-ground-is-resynced`, which diverges, and a
    direct perturbation of all 32 of its observations moves that scenario and
    nothing else.

    `first_difference` closes most of that, and `undetermined` in `sweep`
    closes the rest by refusing to call a field dead when the only scenarios
    carrying its kind were ones a change might not have shown up in.
    """
    return {
        o["name"]: (
            o.get("verdict"), o.get("strict"),
            o.get("subject_obs"), o.get("measured_subject_obs"),
            tuple(o.get("extra_kinds") or ()), tuple(o.get("missing_kinds") or ()),
            o.get("first_difference"),
        )
        for o in outcomes
    }


# --------------------------------------------------------------------------
# mutations
# --------------------------------------------------------------------------


def perturb_value(v):
    """Corrupt a value while keeping it the same JSON type where possible."""
    if isinstance(v, bool):
        return not v
    if isinstance(v, int):
        return v + 1
    if isinstance(v, float):
        return v + 1.0
    if isinstance(v, str):
        return v + "~"
    if isinstance(v, list):
        return v + [0]
    if isinstance(v, dict):
        return {**v, FOREIGN_KIND: 1}
    return 0


def tier_of(killed: list[str], silent: bool = False, discriminating: bool = False) -> int:
    """0 nothing reddens it, 1 only a structural mutation does, 2 its own step does.

    `silent` promotes a `speak` kill for a check that declares its measured
    step silent; `discriminating` is the third route to tier 2 and is not a
    mutation at all — see `MEASURED_SILENT_NOTE` and the module docstring.
    """
    if discriminating:
        return 2
    if not killed:
        return 0
    strong = [k for k in killed if k.split(":")[0] in EDIT]
    if silent:
        strong += [k for k in killed if k == "speak"]
    return 2 if strong else 1


MEASURED_SILENT_NOTE = "baseline: silent at its measured step where vanilla is not"

# The other promotion, which was not being recorded at all.
#
# The module docstring and the ledger's own header both say there are "two
# narrow promotions to tier 2, both recorded in `why`". Only one of them was.
# `571-a-second-edit-of-the-same-sign-is-ignored` and
# `581-a-second-swing-inside-the-animation-is-swallowed` sit in the ledger as
# `killed_by = ["speak", "drop-all"], tier = 2` with no `why` at all — a tier
# their own `killed_by` does not imply, unexplained, and accepted by `--check`
# because it reads `tier` and takes it.
#
# Both are legitimate: each declares `measured_step = "silent"`, so its whole
# claim is "nothing here" and `speak` is the only question that can falsify it.
# Writing that down is the difference between an audited promotion and an
# unaudited one, and `--check` re-derives it from the scenario FILE rather than
# believing the note.
DECLARED_SILENT_NOTE = ("promoted: the check declares its measured step silent, "
                        "so `speak` is the only mutation that can falsify it")


def discriminates_at_measured_step(rec: dict) -> bool:
    """Is this scenario ALREADY red, at the step it is named for?

    A check that is failing cannot be a check that cannot fail, and the only
    reason this needs stating is that such a check is also the one the mutation
    battery cannot reach: `604-container-click-swap` contributes **zero**
    projected observations at its measured step, because the feature it names
    is not implemented yet, so there is nothing there to drop, retype or
    perturb. Refusing it would refuse a check for correctly reporting a gap,
    and would block the branch that closes the gap.

    Narrow on purpose: the subject silent at the measured step, vanilla not,
    and the scenario red. Anything wider would let a matching scenario claim
    this route.
    """
    return (rec.get("verdict") != "match"
            and rec.get("measured_subject_obs") == 0
            and (rec.get("measured_reference_obs") or 0) > 0)


def scenario_obs(doc: dict, name: str) -> list[dict] | None:
    for s in doc["scenarios"]:
        if s["name"] == name:
            return s["obs"]
    return None


def mutate(trace: dict, name: str, step: int, how: str, field: str | None,
           observe: list[str] | None = None):
    """Return a mutated deep copy of `trace`, or None if the mutation cannot
    apply here — which is a fact about the scenario, not a failure.

    **Only the mutated scenario is copied.** Every branch below edits the
    `obs` of the scenario called `name` and nothing else, so the rest of the
    trace is shared with the caller rather than deep-copied: the document
    serialises to the same JSON, and the copy stops costing 0.1 s of a run
    whose comparator takes 0.2 s (11,211 observations, 2026-10-02)."""
    doc = dict(trace)
    doc["scenarios"] = [copy.deepcopy(s) if s.get("name") == name else s
                        for s in trace["scenarios"]]
    obs = scenario_obs(doc, name)
    if obs is None:
        return None
    at = [o for o in obs if o.get("step") == step]

    if how == "drop-measured":
        if not at:
            return None
        keep = [o for o in obs if o.get("step") != step]
        _replace(doc, name, keep)
    elif how == "drop-all":
        if not obs:
            return None
        _replace(doc, name, [])
    elif how == "retype":
        if not at:
            return None
        for o in at:
            o["kind"] = FOREIGN_KIND
    elif how == "inject":
        # **Duplicate an observation the check's projection SEES.** This
        # duplicated `at[0]` — the first row at the step, whatever its kind —
        # and that was harmless while the recorder dropped a fifth of the
        # protocol at record time. Once `recorder-housekeeping` recorded
        # those families instead, the first row at a measured step was often
        # a `set_time`, a heartbeat or a `chunk_no_block_entities` the
        # declared projection drops, and the duplicate landed outside it:
        # re-swept 2026-09-24 over this branch's own recording pair,
        # seventeen tier-2 checks lost `inject` from `killed_by` and nothing
        # about any of them had changed. Same reasoning as `speak` below.
        kinds = set(observe) if observe else None
        seen = [o for o in at if kinds is None or o.get("kind") in kinds]
        if not seen:
            return None
        obs.insert(obs.index(seen[0]), copy.deepcopy(seen[0]))
    elif how == "reorder":
        if len(at) < 2:
            return None
        idx = [i for i, o in enumerate(obs) if o.get("step") == step]
        rev = list(reversed([obs[i] for i in idx]))
        for i, o in zip(idx, rev):
            obs[i] = o
    elif how == "perturb":
        hit = [o for o in at if field in o.get("fields", {})]
        if not hit:
            return None
        for o in hit:
            o["fields"][field] = perturb_value(o["fields"][field])
    elif how == "speak":
        # Put an observation the check's own projection will SEE where it
        # currently sees nothing.
        #
        # **The trigger is an empty projection, not an empty trace row, and
        # the difference is the whole value of this mutation.** The first
        # draft fired only when the measured step had no observations at all,
        # and `604-container-click-swap` duly scored tier 1: its step 3 holds
        # eight rows, every one of them a `block_event` the check does not
        # name or a `move_entity_heartbeat` that `compare::project` drops
        # unless a check names it. So the step looked noisy and was silent —
        # `measured_subject_obs = 0` — and every edit-an-observation mutation
        # landed outside the projection and moved nothing. Reading that as
        # "this check cannot go red" would have been this script committing
        # the defect it exists to refuse.
        kinds = set(observe) if observe else None
        if [o for o in at if kinds is None or o.get("kind") in kinds]:
            return None
        template = next((o for o in obs if kinds is None or o.get("kind") in kinds), None)
        if template is not None:
            extra = copy.deepcopy(template)
        elif obs:
            extra = copy.deepcopy(obs[0])
            if observe:
                extra["kind"] = observe[0]
        else:
            extra = {"who": "actor", "at_ms": 1, "fields": {},
                     "kind": (observe or [FOREIGN_KIND])[0]}
        extra["step"] = step
        after = [i for i, o in enumerate(obs) if o.get("step", -1) < step]
        obs.insert(after[-1] + 1 if after else 0, extra)
    else:
        die(f"unknown mutation {how!r}")
    return doc


def _replace(doc: dict, name: str, obs: list[dict]) -> None:
    for s in doc["scenarios"]:
        if s["name"] == name:
            s["obs"] = obs


def perturb_kind_field(trace: dict, kind: str, field: str, which: str) -> dict:
    """Corrupt `field` in observations of `kind`. See the module docstring.

    `which` is `"all"` or `"last"`, and **a field is dead only if it survives
    both**. Neither strategy alone is a detector:

    * `"all"` is a *uniform additive offset* for a session counter, which
      `compare::rebase_session_counters` deliberately and correctly discards,
      so `block_changed_ack.seq` and the container `window` cancel inside the
      comparator and read as dead when they are read;
    * `"last"` leaves the earlier observations intact, so a field consumed by
      an aggregate built from all of them — the entity position table
      `compare::project` accumulates before any filter runs — is unmoved, and
      `add_entity.at` reads as dead when it is read.

    Measured on the 106-scenario corpus, 2026-08-13: `"all"` alone reports 11
    dead fields, `"last"` alone reports 32, and the two lists are not nested.
    Only their intersection is a claim about the corpus rather than about the
    mutation strategy.

    Copies only the scenarios that carry `kind`, for the reason `mutate` does.
    """
    doc = dict(trace)
    doc["scenarios"] = [copy.deepcopy(s) if any(o.get("kind") == kind for o in s["obs"])
                        else s for s in trace["scenarios"]]
    for s in doc["scenarios"]:
        hit = [o for o in s["obs"] if o.get("kind") == kind and field in o.get("fields", {})]
        for o in (hit if which == "all" else hit[-1:]):
            o["fields"][field] = perturb_value(o["fields"][field])
    return doc


# --------------------------------------------------------------------------
# the sweep
# --------------------------------------------------------------------------


# **Every comparator run in a sweep is independent of every other**, given the
# baseline: each takes one mutated copy of the subject, and the fold over the
# results reads them in the order the serial loop produced them. So they run
# on a pool, and the ledger is the same file the serial loop writes — byte for
# byte, which `ci/unfalsifiable-falsify.py` was shown against on 2026-10-02
# over train-g's 360-scenario pair (one sweep, 0.35 s a run, 16 min serial).
#
# Capped at a third of the box, at most 8: a sweep runs inside T0, and a gate
# runs other things beside it. `CORPUS_FALSIFIABLE_JOBS=1` is the serial loop
# — the same job function, called in-process — for a reader who wants to see
# the two agree.
def sweep_jobs() -> int:
    env = os.environ.get("CORPUS_FALSIFIABLE_JOBS", "")
    if env.strip():
        try:
            return max(1, int(env))
        except ValueError:
            die(f"CORPUS_FALSIFIABLE_JOBS={env!r} is not an integer")
    return max(1, min(8, (os.cpu_count() or 1) // 3))


# The pool's state. Set before the pool forks, so each worker inherits the
# parsed subject rather than re-reading 4 MB per job.
_JOB: dict = {}


def scenario_encodings(trace: dict) -> dict:
    """`json.dumps` of every scenario, keyed by the object's identity, for
    `encode_trace` to reuse. Taken once, before the pool forks."""
    return {id(sc): (sc, json.dumps(sc)) for sc in trace.get("scenarios", [])}


def encode_trace(doc: dict, enc: "dict | None" = None) -> str:
    """`json.dumps(doc)`, byte for byte, without re-encoding a scenario
    `mutate` did not copy: 23 ms of every comparator run on train-g's pair
    re-encoded 359 untouched scenarios. A scenario is reused only when it IS
    the object that was encoded; the selftest holds the bytes equal."""
    if not enc:
        return json.dumps(doc)
    parts = []
    for k, v in doc.items():
        if k == "scenarios" and isinstance(v, list):
            body = ", ".join(e[1] if (e := enc.get(id(sc))) is not None and e[0] is sc
                             else json.dumps(sc) for sc in v)
            parts.append(f"{json.dumps(k)}: [{body}]")
        else:
            parts.append(f"{json.dumps(k)}: {json.dumps(v)}")
    return "{" + ", ".join(parts) + "}"


def _sig_job(job: tuple) -> "dict[str, tuple] | None":
    """One comparator run: the signature of one mutated subject, or None when
    the mutation does not apply. Each process writes its own files."""
    what, args = job
    if what == "mutate":
        m = mutate(_JOB["subject"], *args)
        if m is None:
            return None
    else:
        m = perturb_kind_field(_JOB["subject"], *args)
    tag = os.getpid()
    mp = _JOB["work"] / f"m-{tag}.json"
    mp.write_text(encode_trace(m, _JOB.get("enc")))
    out = _JOB["work"] / f"r-{tag}.json"
    serve = _serve()
    if serve is None:
        return signature(run_compare(_JOB["binary"], _JOB["checks"], _JOB["ref"], mp, out))
    out.unlink(missing_ok=True)
    serve.stdin.write(f"{mp}\t{out}\n")
    serve.stdin.flush()
    reply = serve.stdout.readline().strip()
    if reply != "ok" or not out.exists():
        die(f"compare-serve wrote no report ({reply or 'no reply'})")
    return signature(json.loads(out.read_text())["outcomes"])


def serve_supported(binary: Path, checks: Path, ref: Path) -> bool:
    """Whether `binary` has `compare-serve`. A tree's release binary can
    predate it — T0 uses whatever `target/release/oracle-runner` is there —
    and one that does prints its usage and exits non-zero on an empty
    request stream, where the real thing exits 0. Without it every run is a
    `compare` process, as before."""
    try:
        r = subprocess.run([str(binary), "compare-serve", str(checks), "--reference", str(ref)],
                           input="", capture_output=True, text=True, timeout=120)
    except (OSError, subprocess.TimeoutExpired):
        return False
    return r.returncode == 0


def _serve():
    """This process's own `compare-serve`, started on first use — after the
    fork, so no two processes ever share one."""
    if "serve" not in _JOB:
        _JOB["serve"] = None
        if _JOB.get("use_serve"):
            _JOB["serve"] = subprocess.Popen(
                [str(_JOB["binary"]), "compare-serve", str(_JOB["checks"]),
                 "--reference", str(_JOB["ref"])],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                text=True)
    return _JOB["serve"]


def sweep(binary: Path, checks: Path, ref: Path, sub: Path, ledger: Path | None,
          corpus: "Corpus | None" = None, axes: str = "AB",
          jobs: "int | None" = None) -> int:
    """`axes` is `"AB"` for every caller but one. `ci/unfalsifiable-falsify.py`
    asks for `"B"` over a pair whose only change is a field implanted for
    axis B to find, and for `"A"` over a corpus cut to the scenarios a
    silenced step can reach; each case reads nothing of the other axis. A
    ledger swept without axis A has no rows, so `--check` refuses every
    scenario in it as UNREGISTERED; one swept without axis B lists no dead
    field, which `--check` refuses against any corpus that pins one."""
    corp = corpus if corpus is not None else corpus_for(checks, ledger)
    names = load_checks(checks)
    work = Path(tempfile.mkdtemp(prefix="corpus-falsifiable-"))
    subject = json.loads(sub.read_text())
    out = work / "r.json"
    jobs = sweep_jobs() if jobs is None else max(1, jobs)

    base_outcomes = run_compare(binary, checks, ref, sub, out)
    base = signature(base_outcomes)
    by_name = {o["name"]: o for o in base_outcomes}
    print(f"== corpus     {checks}  ({len(names)} scenarios)")
    print(f"== reference  {ref}")
    print(f"== subject    {sub}")
    print(f"== baseline   {len(base)} outcomes")

    _JOB.clear()
    _JOB.update(subject=subject, work=work, binary=binary, checks=checks, ref=ref,
                enc=scenario_encodings(subject),
                use_serve=serve_supported(binary, checks, ref))
    pool = None
    if jobs > 1:
        import concurrent.futures
        import multiprocessing
        pool = concurrent.futures.ProcessPoolExecutor(
            max_workers=jobs, mp_context=multiprocessing.get_context("fork"))

    def sigs(batch: list[tuple]) -> list:
        """Results in the order of `batch`, whichever way they were run."""
        if pool is None:
            return [_sig_job(j) for j in batch]
        return list(pool.map(_sig_job, batch, chunksize=4))

    try:
        return _sweep_body(binary, checks, ref, sub, ledger, corp, names, subject, base,
                           by_name, sigs, axes, jobs)
    finally:
        if pool is not None:
            pool.shutdown(wait=True, cancel_futures=True)
        serve = _JOB.pop("serve", None)          # the serial path's, if it ran
        if serve is not None:
            serve.stdin.close()
            serve.wait()
        # Scratch only — one report and one mutated subject per process, ~4 MB
        # each — and every sweep left it in /tmp until this line.
        import shutil
        shutil.rmtree(work, ignore_errors=True)


def _sweep_body(binary, checks, ref, sub, ledger, corp, names, subject, base, by_name,
                sigs, axes, jobs) -> int:
    print(f"== {jobs} comparator process(es), axes {axes}, "
          f"{'compare-serve' if _JOB.get('use_serve') else 'one compare per run'}")

    # ---- axis A ----------------------------------------------------------
    entries: list[dict] = []
    unswept: list[str] = []
    runs = 0
    reference = json.loads(ref.read_text())
    missing: list[tuple[str, str, str]] = []       # (name, file, what is absent)
    # Planned in one pass and run as one batch, then folded in the order the
    # plan was made: see `sweep_jobs`.
    planned: list[tuple] = []
    batch: list[tuple] = []
    for name, (path, doc) in (sorted(names.items()) if "A" in axes else ()):
        rec = by_name.get(name)
        if rec is None or scenario_obs(subject, name) is None:
            # **NO ROW IS WRITTEN. 2026-09-05.**
            #
            # This used to append an entry with an empty `killed_by`, which
            # `write_ledger` then rendered as `tier = 0` — a positive claim,
            # in the ledger's own vocabulary, that no mutation of the subject
            # can redden this check. The evidence for that claim was that the
            # sweep never ran a single mutation against it, because the
            # recording pair does not contain the scenario at all.
            #
            # "Not measured" and "measured and found unfalsifiable" are the two
            # readings this whole file exists to keep apart, and the ledger was
            # writing the second where it meant the first. It cost the items and
            # player-systems lanes a turn each: a lane worktree whose
            # `oracle-work/gameplay` recordings predate its own new scenarios
            # cannot reach tier 2 from them, and sweeping filed the new checks
            # at tier 0 and rewrote `floor`, `dead_fields` and `field_universe`
            # from a partial corpus while it was there.
            #
            # An absent row reads as UNREGISTERED on `--check`, which is the
            # true statement: this check ships with no red-when-broken
            # evidence. `[meta] unswept` still names it, and `--check` tolerates
            # a *missing* row for a name declared there, up to MAX_UNSWEPT.
            unswept.append(name)
            in_ref = scenario_obs(reference, name) is not None
            in_sub = scenario_obs(subject, name) is not None
            missing.append((name, path.name,
                            f"{'' if in_ref else str(ref) + ' '}"
                            f"{'' if in_sub else str(sub) + ' '}".strip()
                            or f"neither trace records observations for it "
                               f"(the comparator produced no outcome)"))
            continue

        # Every scenario is mutated, including one that is already `diverge`.
        # An earlier draft short-circuited those as "observed red, evidence
        # enough" — but a diverging scenario proves the *server* is wrong
        # somewhere in its box, not that the *check* reads its measured step,
        # and those are the two different claims this whole file is about. The
        # signature is richer than the verdict, so a mutation moves a diverging
        # scenario just as visibly as a matching one.
        step = rec.get("measured_step")
        observe = doc.get("observe") if isinstance(doc.get("observe"), list) else None
        plan: list[tuple[str, str | None]] = [
            ("drop-measured", None), ("retype", None), ("reorder", None),
            ("inject", None), ("speak", None), ("drop-all", None),
        ]
        for f in sorted({k for o in (scenario_obs(subject, name) or [])
                         if o.get("step") == step for k in o.get("fields", {})}):
            plan.append(("perturb", f))

        planned.append((name, path, doc, rec, plan, len(batch)))
        batch += [("mutate", (name, step, how, field, observe)) for how, field in plan]

    results = sigs(batch)
    for name, path, doc, rec, plan, at in planned:
        killed: list[str] = []
        localised = False
        for (how, field), s in zip(plan, results[at:at + len(plan)]):
            if s is None:
                continue
            runs += 1
            if s.get(name) == base.get(name):
                continue
            label = how if field is None else f"perturb:{field}"
            killed.append(label)
            moved = {n for n in base if s.get(n) != base.get(n)}
            if moved == {name}:
                localised = True

        silent = doc.get("measured_step") == "silent"
        disc = discriminates_at_measured_step(rec)
        tier = tier_of(killed, silent, disc)
        entries.append({
            "name": name, "file": path.name, "digest": semantic_digest(doc),
            "killed_by": killed, "localised": localised, "tier": tier,
            "why": MEASURED_SILENT_NOTE if disc else
                   DECLARED_SILENT_NOTE if silent and tier == 2 else
                   ("" if tier == 2 else
                    "no mutation moved this outcome" if not killed else
                    "only a structural mutation moves this outcome"),
        })

    # ---- axis B ----------------------------------------------------------
    universe = sorted({(o["kind"], f) for s in subject["scenarios"]
                       for o in s["obs"] for f in o.get("fields", {})})
    # Which scenarios carry each kind, and were they `match` at baseline? A
    # projection change always flips a `match` off `match`, so a field carried
    # only by matching scenarios is DETECTABLE and a null result means dead.
    # A field carried by a scenario that already diverges is not: the verdict
    # cannot move further and the counts need not change. That is
    # `undetermined`, and it is reported apart from `dead` rather than folded
    # into it — the difference is a claim about the corpus against a claim
    # about this instrument's reach.
    carriers: dict[str, set[str]] = {}
    for s in subject["scenarios"]:
        for o in s["obs"]:
            carriers.setdefault(o.get("kind", ""), set()).add(s["name"])

    dead: list[tuple[str, str]] = []
    undetermined: list[tuple[str, str]] = []
    # Two rounds, so `"last"` runs only where `"all"` moved nothing — the
    # short-circuit the serial `all(...)` made, kept.
    first = sigs([("perturb", (kind, field, "all"))
                  for kind, field in (universe if "B" in axes else ())])
    still = [kf for kf, sg in zip(universe, first) if sg == base]
    second = dict(zip(still, sigs([("perturb", (kind, field, "last"))
                                   for kind, field in still])))
    for kind, field in (universe if "B" in axes else ()):
        runs += 2
        if second.get((kind, field)) != base:
            continue
        detectable = all(base.get(n, (None,))[0] == "match"
                         and base.get(n, (None, None))[1] == "match"
                         for n in carriers.get(kind, set()) if n in base)
        (dead if detectable else undetermined).append((kind, field))

    # ---- report ----------------------------------------------------------
    swept = [e for e in entries]
    t2 = [e for e in swept if e.get("tier") == 2]
    t1 = [e for e in swept if e.get("tier") == 1]
    t0 = [e for e in swept if e.get("tier") == 0]
    weak = [e for e in swept if e["killed_by"] and not e["localised"]]
    print(f"\n== {runs} comparator runs")
    print(f"axis A  tier 2 (its own step reddens it)    {len(t2)}/{len(swept)} swept "
          f"({len(unswept)} not in the recording)")
    print(f"axis A  tier 1 (only a structural mutation)  {len(t1)}   REFUSED")
    for e in t1:
        print(f"          {e['name']}  ({e['file']})")
    print(f"axis A  tier 0 (nothing reddens it)        {len(t0)}   REFUSED")
    for e in t0:
        print(f"          {e['name']}  ({e['file']})")
    print(f"axis A  no localised kill {len(weak)}")
    for e in weak:
        print(f"          {e['name']}  killed only by {','.join(e['killed_by'])}")
    reference_universe = sorted({(o["kind"], f) for s in json.loads(ref.read_text())["scenarios"]
                                 for o in s["obs"] for f in o.get("fields", {})})
    unaskable = [p for p in reference_universe if p not in set(universe)]
    print(f"axis B  dead fields   {len(dead)}/{len(universe)} (kind, field) pairs "
          f"no check reads")
    for kind, field in dead:
        print(f"          {kind}.{field}")
    print(f"axis B  undetermined {len(undetermined)} — perturbing these moved nothing, but "
          f"every carrier of the kind\n          already diverges, so a change need not "
          f"have shown. NOT a claim either way.")
    for kind, field in undetermined:
        print(f"          {kind}.{field}")
    print(f"axis B  unaskable   {len(unaskable)} of {len(reference_universe)} pairs the "
          f"REFERENCE records and\n          this server never sends, so there is nothing "
          f"to perturb and no check\n          can be asked whether it reads them")
    for kind, field in unaskable:
        print(f"          {kind}.{field}")

    # **What is missing, by name, and which trace it is missing from.** An
    # agent whose sweep quietly dropped rows needs to know which recording to
    # re-take, not that a number went down.
    if missing:
        print(f"\nNO RECORDING PAIR — {len(missing)} scenario(s) get NO ledger row "
              f"(they are not measured, which is not the same as unfalsifiable):")
        for name, fname, what in missing:
            print(f"          {name}  ({fname})\n              absent from: {what}")
        print("          re-record the pair over this scenario directory before sweeping:\n"
              f"              ci/t2-gameplay.sh          (or the pass that owns {checks})\n"
              "          a sweep taken over a recording that predates a scenario cannot\n"
              "          reach tier 2 for it, and must not claim tier 0 on its behalf.")

    if ledger is not None:
        # **A partial sweep may not LOWER the ratchet.** `floor` used to be
        # written as `len(t2)` unconditionally, so the same partial recording
        # that dropped the rows also rewrote the floor down to match — and the
        # ledger came out green while protecting nothing. Carrying the old
        # floor forward makes a partial sweep produce a ledger T0 REFUSES, by
        # name, on both counts. A sweep with a complete pair is unaffected: the
        # branch below is the only one it takes.
        floor = len(t2)
        if missing and ledger.exists():
            try:
                prev = tomllib.loads(ledger.read_text()).get("meta", {}).get("floor")
            except tomllib.TOMLDecodeError:
                prev = None
            if isinstance(prev, int) and prev > floor:
                print(f"\nfloor: keeping {prev} from the existing ledger rather than "
                      f"writing {floor} — {len(missing)} scenario(s) had no recording,\n"
                      f"       so this sweep is not evidence that the tier-2 population "
                      f"shrank.")
                floor = prev
        write_ledger(ledger, entries, unswept, dead, len(universe), floor, len(weak),
                     undetermined, unaskable, len(reference_universe), corp.title,
                     partial=None if axes == "AB" else axes)
        print(f"\nwrote {ledger}")
    return 0


def _q(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def write_ledger(path: Path, entries, unswept, dead, universe, floor, weak,
                 undetermined=(), unaskable=(), reference_universe=None,
                 title=None, partial=None) -> None:
    L = [
        "# " + (title or GAMEPLAY.title),
        "#",
        "# Generated by `ci/corpus-falsifiable.py --sweep`; read by `--check` in",
        "# `ci/t0.sh`. Do not hand-edit an entry: the digest ties a scenario's",
        "# red-when-broken evidence to the version of the scenario it was taken",
        "# against, and a hand-written digest is evidence of nothing.",
        "#",
        "# `killed_by` lists the mutations of the SUBJECT trace that moved this",
        "# scenario's outcome, and `tier` grades them:",
        "#",
        "#   2  an EDIT of the step the scenario is NAMED FOR reddens it — the",
        "#      verdict is coupled to the content of its own claim",
        "#   1  only a STRUCTURAL mutation reddens it (`drop-all`, `speak`) — the",
        "#      check notices traffic and notices silence and nothing in between",
        "#   0  nothing reddens it: the check cannot go red",
        "#",
        "# T0 refuses tier 0 and tier 1 alike. Tier 1 is not a weaker pass, it is",
        "# the shape of a check that scores a claim it never reads.",
        "#",
        "# Two narrow promotions to tier 2, and each records its own `why` — a",
        "# check that DECLARES its measured step silent earns it from `speak`, and",
        "# a check already red at the step it names has had its falsifiability",
        "# shown by the corpus, which beats any mutation.",
        "#",
        "# `--check` REFUSES a tier above the one an entry's own `killed_by`",
        "# implies unless one of those two notes is present, and it re-derives the",
        "# silent one from the scenario file rather than believing the note. The",
        "# digest above cannot do that job: it is taken over the scenario file and",
        "# says nothing about the evidence recorded against it, so it detects",
        "# STALE evidence and not INVENTED evidence.",
        "",
        "[meta]",
        f"battery = [{', '.join(_q(b) for b in BATTERY)}]",
        f"floor = {floor}",
        f"weak_floor = {weak}",
        f"dead_fields = {len(dead)}",
        f"undetermined_fields = {len(undetermined)}",
        f"field_universe = {universe}",
        f"reference_field_universe = {reference_universe if reference_universe is not None else universe}",
        f"unaskable_fields = {len(unaskable)}",
        f"unswept = [{', '.join(_q(n) for n in unswept)}]",
        # Only a sweep that skipped an axis writes this, and `--check` refuses
        # any ledger that carries it: see `sweep`'s `axes`.
        *([f"partial_axes = {_q(partial)}"] if partial else []),
        "",
    ]
    for kind, field in dead:
        L += ["[[dead_field]]", f"kind = {_q(kind)}", f"field = {_q(field)}", ""]
    for kind, field in undetermined:
        L += ["[[undetermined_field]]", f"kind = {_q(kind)}", f"field = {_q(field)}",
              "# every scenario carrying this kind already diverges, so perturbing it",
              "# moving nothing is not evidence that nothing reads it", ""]
    for kind, field in unaskable:
        L += ["[[unaskable_field]]", f"kind = {_q(kind)}", f"field = {_q(field)}",
              "# the reference records this and this server never sends it, so axis B",
              "# has nothing to perturb — not a claim that no check reads it", ""]
    # **The merge marker is part of the file's contract and the sweep was
    # deleting it on every run.** `ci/setup-merge-drivers.sh` lists this ledger
    # among the region files and `ci/merge-docregions.py` resolves its
    # `[[scenario]]` rows with the `toml-scenarios` driver; without the BEGIN
    # line the driver has no region to work in, so a rewritten ledger got the
    # driver's refusals and none of its resolutions. Regenerated here rather
    # than preserved from the old file, because the sweep writes the whole
    # document and a marker that survives only when somebody remembers to
    # copy it forward is a marker that will go again.
    L += ["# MERGE-REGION BEGIN scenarios to-eof check=toml-scenarios"]
    for e in sorted(entries, key=lambda x: x["file"]):
        L += [
            "[[scenario]]",
            f"name = {_q(e['name'])}",
            f"file = {_q(e['file'])}",
            f"digest = {_q(e['digest'])}",
            f"killed_by = [{', '.join(_q(k) for k in e['killed_by'])}]",
            f"tier = {e.get('tier', tier_of(e['killed_by']))}",
            f"localised = {'true' if e['localised'] else 'false'}",
        ]
        if e["why"]:
            L.append(f"why = {_q(e['why'])}")
        L.append("")
    path.write_text("\n".join(L))


# --------------------------------------------------------------------------
# the gate
# --------------------------------------------------------------------------


def check(checks: Path, ledger: Path, corpus: "Corpus | None" = None) -> int:
    # The pins are per corpus and never defaulted — see CORPORA. Passed
    # explicitly only by the selftest, which builds fixture trees.
    corp = corpus if corpus is not None else corpus_for(checks, ledger)
    if not ledger.exists():
        die(f"no ledger at {ledger} — run `ci/corpus-falsifiable.py --sweep "
            f"--checks {corp.checks} --ledger {ledger}`")
    try:
        led = tomllib.loads(ledger.read_text())
    except tomllib.TOMLDecodeError as e:
        die(f"{ledger} does not parse: {e}")
    names = load_checks(checks)
    meta = led.get("meta", {})
    rows = {r["name"]: r for r in led.get("scenario", [])}
    fail: list[str] = []

    if meta.get("partial_axes"):
        fail.append(
            f"PARTIAL     ledger swept axis {meta['partial_axes']} only — it is a "
            f"fragment `ci/unfalsifiable-falsify.py` builds from, not evidence")

    if tuple(meta.get("battery", ())) != BATTERY:
        fail.append(
            f"BATTERY     ledger taken with {list(meta.get('battery', []))}, "
            f"this instrument mutates with {list(BATTERY)} — re-sweep")

    unswept_declared = set(meta.get("unswept", []))
    if len(unswept_declared) > corp.max_unswept:
        fail.append(
            f"UNSWEPT     {len(unswept_declared)} scenarios have no evidence, "
            f"allowance is {corp.max_unswept} (MAX_UNSWEPT in ci/corpus-falsifiable.py)")

    for name, (path, doc) in sorted(names.items()):
        r = rows.get(name)
        if r is None:
            # A name the ledger declares `unswept` has NO row since 2026-09-05
            # — the sweep refuses to write one rather than filing tier 0 for a
            # scenario no recording contains. That is tolerated here, up to
            # MAX_UNSWEPT, which is checked above and in code rather than data.
            if name in unswept_declared:
                continue
            fail.append(f"UNREGISTERED {name} ({path.name}) — no red-when-broken evidence")
            continue
        want = semantic_digest(doc)
        if r.get("digest") != want:
            fail.append(f"STALE       {name} ({path.name}) changed since its evidence "
                        f"was taken — re-sweep")
            continue
        if name in unswept_declared and not r.get("killed_by"):
            continue
        killed = list(r.get("killed_by", []))
        derived = tier_of(killed)
        tier = r.get("tier", derived)
        # **A recorded tier above the one its own `killed_by` implies has to
        # say which promotion it is, and the silent one is re-derived here
        # rather than believed.** `--check` used to read `tier` and take it, so
        # a hand-written `tier = 2` beside a structural-only kill passed — the
        # ledger's header forbids hand-editing and its digest cannot detect it,
        # because the digest is over the SCENARIO FILE and says nothing about
        # the evidence recorded against it.
        if tier > derived:
            why = r.get("why", "")
            doc_silent = (names[name][1].get("measured_step") == "silent"
                          if name in names else False)
            if why == MEASURED_SILENT_NOTE:
                pass                      # already red at its measured step
            elif why == DECLARED_SILENT_NOTE and doc_silent and "speak" in killed:
                pass                      # the check declares its step silent
            else:
                fail.append(
                    f"PROMOTED    {name} records tier {tier} where its killed_by "
                    f"{killed} implies {derived}"
                    + (f", and {r.get('file')} does not declare measured_step = \"silent\""
                       if why == DECLARED_SILENT_NOTE and not doc_silent
                       else f", with why = {why!r}"))
                continue
        if tier == 0:
            fail.append(f"UNFALSIFIABLE {name} ({path.name}) — no mutation of the subject "
                        f"moves this outcome; it cannot go red")
        elif tier == 1:
            fail.append(f"UNFALSIFIABLE {name} ({path.name}) — only deleting the whole "
                        f"scenario's output reddens it; nothing about the step it is "
                        f"named for is coupled to its verdict")

    for name, r in sorted(rows.items()):
        if name not in names:
            fail.append(f"ORPHAN      ledger entry {name} ({r.get('file')}) has no scenario file")

    live = sum(1 for n, r in rows.items()
               if n in names and r.get("tier", tier_of(list(r.get("killed_by", [])))) == 2)
    floor = meta.get("floor")
    if isinstance(floor, int) and live < floor:
        fail.append(f"RATCHET     {live} tier-2 scenarios, floor is {floor} — "
                    f"a check stopped being able to fail")

    weak = sum(1 for n, r in rows.items()
               if n in names and r.get("killed_by") and not r.get("localised"))
    weak_floor = meta.get("weak_floor")
    if isinstance(weak_floor, int) and weak > weak_floor:
        fail.append(f"RATCHET     {weak} scenarios have no localised kill, ceiling is "
                    f"{weak_floor} — a check went vague about what it measures")

    # `undetermined_fields` is recorded in the ledger and deliberately NOT
    # ratcheted. It rises when more scenarios diverge — a fact about the
    # server, not about the checks — and a gate that reddens because our
    # server got worse in some unrelated place is a gate agents learn to
    # bypass. It is there to be read, and to stop `dead_fields` being mistaken
    # for the whole population.
    # **Against the pinned set in this file, not against the count beside it.**
    # See `DEAD_FIELDS`: the two numbers in the ledger are equal by
    # construction and the refusal below could never fire.
    dead_set = {(d.get("kind"), d.get("field")) for d in led.get("dead_field", [])}
    dead = len(led.get("dead_field", []))
    pinned = set(corp.dead_fields)
    if dead_set != pinned:
        added = ", ".join(f"{k}.{f}" for k, f in sorted(dead_set - pinned))
        gone = ", ".join(f"{k}.{f}" for k, f in sorted(pinned - dead_set))
        fail.append("DEAD-FIELDS the recorded fields no check reads are not the pinned set"
                    + (f" — new: {added}" if added else "")
                    + (f" — no longer dead: {gone}" if gone else "")
                    + " (DEAD_FIELDS in ci/corpus-falsifiable.py)")
    if meta.get("dead_fields") != len(pinned):
        fail.append(f"DEAD-FIELDS [meta] says dead_fields = {meta.get('dead_fields')} and "
                    f"{len(pinned)} are pinned in ci/corpus-falsifiable.py")
    # **Ratcheted upward only.** A pair the reference records and this server
    # never sends cannot be asked about by perturbation; the set shrinking is
    # this server implementing something, which must not redden a gate, and it
    # growing is a new scenario exposing a packet we do not send.
    unaskable = len(led.get("unaskable_field", []))
    if unaskable > corp.max_unaskable:
        fail.append(f"UNASKABLE   {unaskable} (kind, field) pairs are recorded by the "
                    f"reference and never by this server, ceiling is {corp.max_unaskable} "
                    "(MAX_UNASKABLE in ci/corpus-falsifiable.py) — a check was added "
                    "over a packet nothing here sends")
    if meta.get("unaskable_fields") is not None and meta.get("unaskable_fields") != unaskable:
        fail.append(f"UNASKABLE   [meta] says unaskable_fields = {meta.get('unaskable_fields')} "
                    f"and {unaskable} are listed")
    # `field_universe` had no consumer at all until this line. It is the
    # denominator a published ratio is quoted against, so it is checked here or
    # it is not checked anywhere.
    if meta.get("field_universe") != corp.field_universe:
        fail.append(f"FIELD-UNIVERSE ledger says {meta.get('field_universe')} recorded "
                    f"(kind, field) pairs, this file pins {corp.field_universe} — the "
                    "denominator of the dead-field ratio moved")

    if fail:
        print("CORPUS-FALSIFIABLE RED", file=sys.stderr)
        for f in fail:
            print("  " + f, file=sys.stderr)
        # Every refusal above is cleared by the same command, so it is printed
        # once, in full, rather than alluded to per line. It needs a recorded
        # trace pair — any tree's `oracle-work/gameplay` has one — and it takes
        # no server, no vanilla boot and NOT the corpus lock.
        rec = ("oracle-work/gameplay/survival" if corp.key == "survival"
               else "oracle-work/gameplay")
        print("\n  to re-take the evidence (about 95s, no lock, no server):\n"
              "    ci/corpus-falsifiable.py --sweep --bin target/release/oracle-runner \\\n"
              f"        --checks {corp.checks.relative_to(ROOT)} "
              f"--ledger {corp.ledger.relative_to(ROOT)} \\\n"
              f"        {rec}/vanilla.json {rec}/rustmc.json\n"
              f"  then commit {corp.ledger.relative_to(ROOT)}.", file=sys.stderr)
        return 1
    print(f"corpus-falsifiable: {live}/{len(names)} {corp.key} checks carry evidence that "
          f"their own measured step can turn them red ({len(unswept_declared)} unswept, "
          f"{dead} dead fields, {weak} without a localised kill)")
    return 0


# --------------------------------------------------------------------------
# selftest — the house pattern; needs no recordings and no server
# --------------------------------------------------------------------------


def selftest() -> int:
    import shutil
    bad: list[str] = []

    # The fixture ledgers below are not any shipped corpus, so the pins are
    # passed rather than looked up — `corpus_for` deliberately refuses an
    # unknown ledger name instead of defaulting to gameplay's.
    _check = globals()['check']

    def check(cdir, led, corpus=GAMEPLAY):      # noqa: F811 — shadows for this scope
        return _check(cdir, led, corpus)

    def want(case: str, ok: bool, detail: str = "") -> None:
        if not ok:
            bad.append(f"{case}: {detail}")

    # perturbation preserves type and changes value
    for v in (True, 3, 3.5, "x", [1, 2], {"a": 1}, None):
        want(f"perturb {type(v).__name__}", perturb_value(v) != v, repr(v))
    want("perturb bool stays bool", isinstance(perturb_value(True), bool))
    want("perturb int stays int", isinstance(perturb_value(3), int))

    # a mutated trace encodes to json.dumps's exact bytes, reused scenarios
    # and all — including the scenario mutate copied and the ones it did not
    tr = {"target": "t", "scenarios": [
        {"name": "a", "anchor": [0, 1, 2], "obs": [{"step": 1, "kind": "k", "fields": {"x": 1}}]},
        {"name": "b", "anchor": [3, 4, 5], "obs": [{"step": 1, "kind": "k", "fields": {"é": [1.5]}},
                                                   {"step": 2, "kind": "j", "fields": {}}]},
        {"name": "c", "anchor": [6, 7, 8], "obs": []}], "version": "v"}
    enc = scenario_encodings(tr)
    for how in ("drop-measured", "retype", "reorder", "inject", "speak", "drop-all"):
        m = mutate(tr, "b", 1, how, None, ["k"])
        if m is not None:
            want(f"encode_trace after {how}", encode_trace(m, enc) == json.dumps(m))
    m = perturb_kind_field(tr, "k", "x", "all")
    want("encode_trace after perturb_kind_field", encode_trace(m, enc) == json.dumps(m))
    want("encode_trace reuses only the encoded object",
         encode_trace(json.loads(json.dumps(tr)), enc) == json.dumps(tr))

    # the semantic digest ignores prose and nothing else
    a = {"name": "x", "about": "one", "observe": ["block_update"]}
    b = {"name": "x", "about": "two", "observe": ["block_update"]}
    c = {"name": "x", "about": "one", "observe": ["sound"]}
    want("digest ignores about", semantic_digest(a) == semantic_digest(b))
    want("digest sees observe", semantic_digest(a) != semantic_digest(c))

    # each mutation actually mutates, and `mutate` copies rather than aliases
    trace = {"scenarios": [{"name": "s", "obs": [
        {"step": 0, "kind": "k", "fields": {"n": 1}},
        {"step": 0, "kind": "k", "fields": {"n": 2}},
        {"step": 1, "kind": "k", "fields": {"n": 3}},
    ]}]}
    frozen = json.dumps(trace, sort_keys=True)
    for how, field, expect in (
        ("drop-measured", None, lambda o: len(o) == 1),
        ("drop-all", None, lambda o: len(o) == 0),
        ("retype", None, lambda o: o[0]["kind"] == FOREIGN_KIND),
        ("inject", None, lambda o: len(o) == 4),
        ("reorder", None, lambda o: o[0]["fields"]["n"] == 2),
        ("perturb", "n", lambda o: o[0]["fields"]["n"] == 2),
    ):
        m = mutate(trace, "s", 0, how, field)
        want(f"mutation {how} applies", m is not None, "returned None")
        if m is not None:
            want(f"mutation {how} changes the trace", expect(m["scenarios"][0]["obs"]))
    want("mutate does not touch its input", json.dumps(trace, sort_keys=True) == frozen)

    # a mutation that cannot apply says so rather than pretending
    want("reorder needs two", mutate(trace, "s", 1, "reorder", None) is None)
    want("perturb needs the field", mutate(trace, "s", 0, "perturb", "zz") is None)
    want("unknown scenario", mutate(trace, "nope", 0, "drop-all", None) is None)

    # `speak` fires on an empty PROJECTION, not an empty trace row
    want("speak declines a step that spoke", mutate(trace, "s", 0, "speak", None) is None)
    m = mutate(trace, "s", 2, "speak", None)
    want("speak fills a silent step", m is not None and len(m["scenarios"][0]["obs"]) == 4)
    if m is not None:
        want("speak inserts after the earlier steps",
             [o["step"] for o in m["scenarios"][0]["obs"]] == [0, 0, 1, 2])
    empty = {"scenarios": [{"name": "s", "obs": []}]}
    m = mutate(empty, "s", 0, "speak", None, ["sound"])
    want("speak synthesises from `observe` when the scenario is silent throughout",
         m is not None and m["scenarios"][0]["obs"][0]["kind"] == "sound")

    # THE `604` CASE: a measured step full of observations the check does not
    # name still counts as silent, because the projection sees none of them.
    noisy = {"scenarios": [{"name": "s", "obs": [
        {"step": 0, "kind": "container_set_slot", "who": "actor", "fields": {"slot": 0}},
        {"step": 3, "kind": "block_event", "who": "actor", "fields": {}},
        {"step": 3, "kind": "move_entity_heartbeat", "who": "actor", "fields": {}},
    ]}]}
    m = mutate(noisy, "s", 3, "speak", None, ["container_set_slot"])
    want("speak fires on a step that is noisy but unprojected", m is not None)
    if m is not None:
        got = [o for o in m["scenarios"][0]["obs"]
               if o["step"] == 3 and o["kind"] == "container_set_slot"]
        want("speak inserts a kind the check names", len(got) == 1)
    want("speak declines when the projection already sees the step",
         mutate(noisy, "s", 0, "speak", None, ["container_set_slot"]) is None)

    # `inject` duplicates a row the projection sees, not merely the first row
    # at the step: a leading ambient row would put the duplicate outside it.
    lead = {"scenarios": [{"name": "s", "obs": [
        {"step": 1, "kind": "set_time", "who": "actor", "fields": {}},
        {"step": 1, "kind": "container_set_slot", "who": "actor", "fields": {"slot": 0}},
    ]}]}
    m = mutate(lead, "s", 1, "inject", None, ["container_set_slot"])
    want("inject duplicates a kind the check names",
         m is not None and [o["kind"] for o in m["scenarios"][0]["obs"]].count("container_set_slot") == 2)
    want("inject declines a step the projection does not see",
         mutate(lead, "s", 1, "inject", None, ["sound"]) is None)

    # both field-perturbation strategies, because a field is dead only if it
    # survives both — see the module docstring on axis B
    m = perturb_kind_field(trace, "k", "n", "last")
    want("perturb_kind_field last moves the last only",
         [o["fields"]["n"] for o in m["scenarios"][0]["obs"]] == [1, 2, 4])
    m = perturb_kind_field(trace, "k", "n", "all")
    want("perturb_kind_field all moves every one",
         [o["fields"]["n"] for o in m["scenarios"][0]["obs"]] == [2, 3, 4])

    # tiers
    want("tier 0 on nothing", tier_of([]) == 0)
    want("tier 1 on drop-all alone", tier_of(["drop-all"]) == 1)
    want("tier 1 on speak alone", tier_of(["speak"]) == 1)
    want("tier 1 on both structural kills", tier_of(["speak", "drop-all"]) == 1)
    want("tier 2 on a measured-step kill", tier_of(["drop-all", "drop-measured"]) == 2)
    want("tier 2 on a perturb", tier_of(["perturb:state_id"]) == 2)
    want("speak counts only for a declared-silent check",
         tier_of(["speak"], silent=True) == 2)
    want("declared-silent does not promote drop-all",
         tier_of(["drop-all"], silent=True) == 1)
    want("a discriminating scenario is tier 2 with no kill at all",
         tier_of([], discriminating=True) == 2)

    # the third route to tier 2, and how narrow it is
    want("604 shape: red, silent at its step, vanilla is not",
         discriminates_at_measured_step(
             {"verdict": "diverge", "measured_subject_obs": 0,
              "measured_reference_obs": 2}))
    want("a matching scenario cannot take that route",
         not discriminates_at_measured_step(
             {"verdict": "match", "measured_subject_obs": 0,
              "measured_reference_obs": 0}))
    want("a red scenario that DID speak at its step cannot take it either",
         not discriminates_at_measured_step(
             {"verdict": "diverge", "measured_subject_obs": 2,
              "measured_reference_obs": 2}))
    want("nor one where vanilla was silent too",
         not discriminates_at_measured_step(
             {"verdict": "diverge", "measured_subject_obs": 0,
              "measured_reference_obs": 0}))

    # the signature separates two reports that differ in anything but prose
    r1 = [{"name": "s", "about": "a", "verdict": "match", "strict": "match",
           "subject_obs": 2, "measured_subject_obs": 2,
           "extra_kinds": [], "missing_kinds": []}]
    r2 = [dict(r1[0], about="b")]
    r3 = [dict(r1[0], subject_obs=3)]
    want("signature ignores about", signature(r1) == signature(r2))
    want("signature sees counts", signature(r1) != signature(r3))

    # --check refuses each way, on a real ledger it writes itself
    tmp = Path(tempfile.mkdtemp(prefix="cf-selftest-"))
    cdir = tmp / "checks"
    cdir.mkdir()
    (cdir / "010-a.toml").write_text('name = "a"\nabout = "x"\nobserve = ["block_update"]\n')
    (cdir / "020-b.toml").write_text('name = "b"\nabout = "y"\nobserve = ["sound"]\n')
    docs = load_checks(cdir)
    good = [
        {"name": "a", "file": "010-a.toml", "digest": semantic_digest(docs["a"][1]),
         "killed_by": ["drop-measured"], "tier": 2, "localised": True, "why": ""},
        {"name": "b", "file": "020-b.toml", "digest": semantic_digest(docs["b"][1]),
         "killed_by": ["retype"], "tier": 2, "localised": True, "why": ""},
    ]
    led = tmp / "l.toml"

    write_ledger(led, good, [], list(DEAD_FIELDS), FIELD_UNIVERSE, 2, 0)
    want("clean ledger passes", check(cdir, led) == 0)

    write_ledger(led, [dict(good[0], killed_by=[], tier=0,
                            why="no mutation moved this outcome"),
                       good[1]], [], list(DEAD_FIELDS), FIELD_UNIVERSE, 1, 0)
    want("UNFALSIFIABLE tier 0 refused", check(cdir, led) == 1)

    # The tier-1 case is the historical defect and the one a count of "some
    # mutation reddened it" would have passed.
    write_ledger(led, [dict(good[0], killed_by=["drop-all"], tier=1), good[1]],
                 [], [], 10, 1, 0)
    want("UNFALSIFIABLE tier 1 refused", check(cdir, led) == 1)

    write_ledger(led, [good[0]], [], list(DEAD_FIELDS), FIELD_UNIVERSE, 1, 0)
    want("UNREGISTERED refused", check(cdir, led) == 1)

    write_ledger(led, [dict(good[0], digest="sha256:0"), good[1]], [], list(DEAD_FIELDS), FIELD_UNIVERSE, 2, 0)
    want("STALE refused", check(cdir, led) == 1)

    write_ledger(led, good + [{"name": "gone", "file": "030-gone.toml", "digest": "sha256:0",
                               "killed_by": ["retype"], "localised": True, "why": ""}],
                 [], [], 10, 2, 0)
    want("ORPHAN refused", check(cdir, led) == 1)

    write_ledger(led, good, [], list(DEAD_FIELDS), FIELD_UNIVERSE, 3, 0)
    want("RATCHET on the floor refused", check(cdir, led) == 1)

    write_ledger(led, [dict(good[0], localised=False), good[1]], [], list(DEAD_FIELDS), FIELD_UNIVERSE, 2, 0)
    want("RATCHET on localisation refused", check(cdir, led) == 1)

    # **The dead-field ratchet, and the reason it was rewritten.**
    #
    # It used to read `dead_fields` out of `[meta]` and compare it against
    # `len(dead_field)` from the same file, written by the same sweep run.
    # Those are equal by construction, so the refusal could not fire; the case
    # below used to build one by editing the ceiling under the population,
    # which is a test of a hand-edit and not of a sweep. It now compares the
    # ledger against `DEAD_FIELDS` in this file, which is the same answer
    # `MAX_UNSWEPT` gives one line up.
    write_ledger(led, good, [], list(DEAD_FIELDS), FIELD_UNIVERSE, 2, 0)
    want("the pinned dead set passes", check(cdir, led) == 0)

    # A sweep that finds a NEW dead field — the case the ratchet exists for and
    # the one it could not previously see, because the sweep would have written
    # a matching ceiling beside it.
    write_ledger(led, good, [], [*DEAD_FIELDS, ("sound", "volume")], FIELD_UNIVERSE, 2, 0)
    want("a newly dead field refused", check(cdir, led) == 1)

    # And a field that came back to life: also a change worth a reviewer, since
    # it moves the denominator of a published ratio in the other direction.
    #
    # **THIS CASE WENT INERT ON 2026-09-05 AND PASSED WHILE INERT.** It was
    # `write_ledger(..., list(DEAD_FIELDS[:-1]), ...)`, and `DEAD_FIELDS`
    # emptied that day (see the constant): `()[:-1]` is `()`, so the case wrote
    # the pinned set unchanged and then asserted a refusal that nothing could
    # raise. An injection that has stopped injecting, reported green — the same
    # shape `ci/command-surface-falsify.py`'s `surface-edited-by-hand` mutator
    # was found in on 2026-09-05, one file over.
    #
    # It is built from a synthetic pinned set now, so the reduction is a real
    # reduction whatever the corpus happens to make dead. The second `want` is
    # the positive control and it is the load-bearing one: if the repin ever
    # stops reaching `check`, the ledger it writes will differ from the *real*
    # pinned set, `check` will refuse, and this case goes RED rather than back
    # to green-and-inert. `origin/corpus-instruments` moves this constant into
    # a `Corpus` record built at import time — when that lands, this repin
    # stops taking, and the control below is what will say so.
    #
    # **AND IT SAID SO — 2026-09-05, rebasing this branch onto that change.**
    # `check` reads `corpus_for(cdir, led).dead_fields`, not this module
    # global, so repinning the global alone left `check` comparing the ledger
    # against the REAL (empty) pinned set while the ledger carried the
    # synthetic one: `the repinned dead set is the one `check` reads` went RED,
    # which is the case working, not the case broken. The repin now moves the
    # record's own attribute — the value `check` actually reads — and the
    # module global with it, so `write_ledger`'s arguments stay derived from
    # one source. Both are restored in `finally`. `GAMEPLAY` is the record
    # because the shadowed `check` in this scope passes `corpus=GAMEPLAY`;
    # the new first `want` is the cheap version of the same control, failing
    # on a repin that never reaches the record without a ledger round trip.
    _pinned = DEAD_FIELDS
    _corp_pinned = GAMEPLAY.dead_fields
    globals()["DEAD_FIELDS"] = (("sound", "volume"), ("sound", "pitch"))
    GAMEPLAY.dead_fields = DEAD_FIELDS
    try:
        want("the repin reaches the record `check` reads",
             list(GAMEPLAY.dead_fields) == list(DEAD_FIELDS))
        want("the reduced-dead-set injection still injects",
             list(DEAD_FIELDS[:-1]) != list(DEAD_FIELDS))
        write_ledger(led, good, [], list(DEAD_FIELDS[:-1]), FIELD_UNIVERSE, 2, 0)
        want("a field that stopped being dead refused", check(cdir, led) == 1)
        write_ledger(led, good, [], list(DEAD_FIELDS), FIELD_UNIVERSE, 2, 0)
        want("the repinned dead set is the one `check` reads", check(cdir, led) == 0)
    finally:
        globals()["DEAD_FIELDS"] = _pinned
        GAMEPLAY.dead_fields = _corp_pinned

    # `field_universe` had no consumer at all before this. A sweep whose
    # recorded vocabulary shrank used to write the smaller number and pass.
    write_ledger(led, good, [], list(DEAD_FIELDS), FIELD_UNIVERSE - 6, 2, 0)
    want("a moved field universe refused", check(cdir, led) == 1)

    # The merge marker the sweep used to delete on every run.
    write_ledger(led, good, [], list(DEAD_FIELDS), FIELD_UNIVERSE, 2, 0)
    want("the merge region marker is regenerated",
         "# MERGE-REGION BEGIN scenarios to-eof check=toml-scenarios" in led.read_text())

    # ---------------------------------------------------------------------
    # NO RECORDING PAIR — 2026-09-05. "Not measured" is not "unfalsifiable".
    #
    # A lane worktree whose recordings predate its own new scenario cannot
    # reach tier 2 for it. The sweep used to file it at tier 0 anyway, which
    # is a positive claim that no mutation can redden it, evidenced by never
    # having run one. The row is now omitted entirely and `[meta] unswept`
    # names it; `--check` tolerates a MISSING row for a declared name, and
    # still refuses a row that RECORDS tier 0.
    # ---------------------------------------------------------------------
    write_ledger(led, [good[0]], ["b"], list(DEAD_FIELDS), FIELD_UNIVERSE, 1, 0)
    want("a declared-unswept scenario with NO row is tolerated", check(cdir, led) == 0)
    want("and the ledger says so rather than scoring it",
         'unswept = ["b"]' in led.read_text() and '"b"' not in
         led.read_text().split("# MERGE-REGION BEGIN")[1])

    # The shape it replaces: the same scenario filed at tier 0 and NOT
    # declared. That is the row the old sweep wrote, and it is refused.
    write_ledger(led, [good[0], {"name": "b", "file": "020-b.toml",
                                 "digest": semantic_digest(docs["b"][1]),
                                 "killed_by": [], "tier": 0, "localised": False,
                                 "why": "not-in-recording"}],
                 [], list(DEAD_FIELDS), FIELD_UNIVERSE, 1, 0)
    want("a tier-0 row for an unrecorded scenario is still refused",
         check(cdir, led) == 1)

    # And the allowance is a real ceiling, in code: more unswept than
    # MAX_UNSWEPT is a refusal however the rows are (not) written.
    write_ledger(led, [good[0]], [f"n{i}" for i in range(MAX_UNSWEPT + 1)] + ["b"],
                 list(DEAD_FIELDS), FIELD_UNIVERSE, 1, 0)
    want("unswept beyond MAX_UNSWEPT refused even with no rows", check(cdir, led) == 1)
    want("the marker precedes the first row it governs",
         led.read_text().index("MERGE-REGION BEGIN") < led.read_text().index("[[scenario]]"))

    write_ledger(led, good, [], list(DEAD_FIELDS), FIELD_UNIVERSE, 2, 0)
    txt = led.read_text().replace('battery = ["drop-measured"',
                                  'battery = ["something-else"')
    led.write_text(txt)
    want("BATTERY mismatch refused", check(cdir, led) == 1)

    # a sweep that skipped an axis is refused whatever else it says — with
    # this fixture's empty dead set an axis-A-only ledger is otherwise green
    write_ledger(led, good, [], list(DEAD_FIELDS), FIELD_UNIVERSE, 2, 0)
    want("partial control: the same ledger without the marker passes",
         check(cdir, led) == 0)
    write_ledger(led, good, [], list(DEAD_FIELDS), FIELD_UNIVERSE, 2, 0, partial="A")
    want("a partial-axes ledger refused", check(cdir, led) == 1)

    # the unswept allowance covers an empty `killed_by`, and only up to the cap
    write_ledger(led, [dict(good[0], killed_by=[], tier=0, why="not-in-recording"), good[1]],
                 ["a"], list(DEAD_FIELDS), FIELD_UNIVERSE, 1, 0)
    want("declared unswept tolerated", check(cdir, led) == 0)
    write_ledger(led, [dict(good[0], killed_by=[], tier=0, why="not-in-recording"), good[1]],
                 [f"n{i}" for i in range(MAX_UNSWEPT + 1)], list(DEAD_FIELDS),
                 FIELD_UNIVERSE, 1, 0)
    want("unswept beyond the cap refused", check(cdir, led) == 1)

    shutil.rmtree(tmp, ignore_errors=True)
    if bad:
        print("CORPUS-FALSIFIABLE SELFTEST FAILED", file=sys.stderr)
        for f in bad:
            print("  " + f, file=sys.stderr)
        return 1
    print("corpus-falsifiable selftest: ok")
    return 0


# --------------------------------------------------------------------------


USAGE = """usage:
  ci/corpus-falsifiable.py --check   [--corpus NAME | --checks DIR --ledger FILE]
  ci/corpus-falsifiable.py --sweep   [--corpus NAME | --checks DIR --ledger FILE]
                                     [--bin PATH] [--no-write] VANILLA.json RUSTMC.json
  ci/corpus-falsifiable.py --selftest

  --corpus NAME   one of: gameplay (default), survival. Sets --checks and
                  --ledger together, which is the only pairing whose pins
                  (dead-field set, field universe) belong to each other."""


def main(argv: list[str]) -> int:
    args = argv[1:]
    if not args:
        print(USAGE, file=sys.stderr)
        return 2
    binary, checks, ledger = DEFAULT_BIN, DEFAULT_CHECKS, DEFAULT_LEDGER
    # `--corpus` first: it sets the pair, and an explicit `--checks`/`--ledger`
    # after it still wins, which is what the selftest and a fixture tree need.
    while "--corpus" in args:
        i = args.index("--corpus")
        if i + 1 >= len(args):
            die("--corpus needs a value")
        want = args[i + 1]
        hit = CORPORA.get(want)
        if hit is None:
            die(f"unknown corpus {want!r} — known: {', '.join(sorted(CORPORA))}")
        checks, ledger = hit.checks, hit.ledger
        del args[i:i + 2]
    for flag, slot in (("--bin", "b"), ("--checks", "c"), ("--ledger", "l")):
        while flag in args:
            i = args.index(flag)
            if i + 1 >= len(args):
                die(f"{flag} needs a value")
            val = Path(args[i + 1])
            binary, checks, ledger = (
                (val, checks, ledger) if slot == "b" else
                (binary, val, ledger) if slot == "c" else (binary, checks, val))
            del args[i:i + 2]
    write = "--no-write" not in args
    args = [a for a in args if a != "--no-write"]

    # **A ledger paired with another corpus's scenarios is refused.** The pins
    # are looked up by ledger name, so `--checks ci/checks/gameplay --ledger
    # ci/checks/survival-falsifiable.toml` would grade 108 creative scenarios
    # against the survival dead-field set and survival's denominator — and the
    # sweep would then WRITE that, giving both corpora a ledger about neither.
    if args and args[0] in ("--check", "--sweep") and ledger.name in BY_LEDGER:
        want_checks = BY_LEDGER[ledger.name].checks
        if checks.resolve() != want_checks.resolve():
            die(f"--ledger {ledger.name} is the {BY_LEDGER[ledger.name].key} corpus and "
                f"its pins belong to {want_checks.relative_to(ROOT)},\n"
                f"         but --checks says {checks}. Use --corpus "
                f"{BY_LEDGER[ledger.name].key}.")

    if args[0] == "--selftest":
        return selftest()
    if args[0] == "--check":
        if len(args) != 1:
            die(USAGE)
        return check(checks, ledger)
    if args[0] == "--sweep":
        if len(args) != 3:
            die(USAGE)
        ref, sub = Path(args[1]), Path(args[2])
        for p in (binary, checks, ref, sub):
            if not p.exists():
                die(f"missing {p}")
        return sweep(binary, checks, ref, sub, ledger if write else None)
    print(USAGE, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
