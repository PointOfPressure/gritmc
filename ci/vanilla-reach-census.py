#!/usr/bin/env python3
"""vanilla-reach-census.py — how much of VANILLA the gameplay corpus touches at all.

    ./ci/vanilla-reach-census.py --explain
    ./ci/vanilla-reach-census.py --recordings '~/rustmc-cov2/oracle-work/gameplay-selfcheck/r[0-9]*.json'

# The question, and why the corpus score cannot answer it

`ci/t2-gameplay.sh` reports something like `72/92 declared`. Ninety-two is the
number of scenarios **we wrote**. It is not a measure of vanilla, so the score
says how much of our own corpus we pass and nothing whatever about how much of
vanilla the corpus even looks at. Asked "how far from finished", the honest
answer before this script was *there is no denominator*.

This script produces denominators, and the one rule it exists to obey is that
**not one of them may come from our tree**. Every count below is read out of
the pinned jar — from Mojang's own `generated/reports/` output, from
`GameProtocols`' own packet registration, and from the profiler section names
Mojang compiled into the server's tick path. Our tree supplies numerators only,
and the numerators are *measurements* off recordings rather than readings of
our source.

# Four units, deliberately, because one number with no sensitivity is worth less

The trap this project falls into is picking the unit that flatters. So:

    protocol   clientbound play packet types            (141, GameProtocols)
    tick       named per-tick profiler sections         (jar, MinecraftServer.tickServer)
    registry   entity types, blocks, block entities,
               menus, sound events                      (jar, reports/registries.json)
    behaviour  see ci/corpus-surface.py's block tables  (jar-derived, printed there)

Read across them and the corpus's "coverage of vanilla" moves by more than an
order of magnitude. That spread **is** the finding. Any single one of these
numbers quoted alone is a choice of unit, and the choice is worth more than the
measurement.

# What each unit cannot say

* **protocol.** A packet observed once is not a system tested. The strongest
  reading — 29 of the 36 types vanilla actually emitted are named by a check —
  is a ratio whose denominator the corpus itself produced: it counts only what
  a creative superflat with two players and no mobs can provoke. It cannot see
  a mechanic whose packets nothing in the corpus causes to be sent, and by
  construction it scores those as absent from the denominator rather than as
  gaps. It flatters, and it is included because a reader will otherwise
  reconstruct it and quote it without the caveat.
* **tick.** The denominator is Mojang's own decomposition and is honest. The
  numerator is not a liveness measurement: nothing here runs a server. Two
  sharp negatives are available and used — a section vanilla's own gate turns
  off under the harness's declared world configuration, and a per-mob section
  for a mob no recording ever contained — and everything else is `unclassified`,
  which means *unknown*, not *covered*.
* **registry.** Sharp on both sides and the most brutal. Its weakness is that
  a registry entry is not a mechanism: 1,968 sound events are not 1,968
  systems, and reaching 3% of blocks does not mean 3% of block behaviour,
  because most blocks share a class. `ci/corpus-surface.py` scores the
  behaviour tables by class as well as by block for exactly that reason, and
  the two answers differ by 4x.
* **all of them.** Every numerator is measured over one ensemble of six vanilla
  recordings from one harness on one world. A mechanism the harness has no verb
  for is invisible to all four units, and that is not conservative — it is a
  blind spot none of these numbers can price.

# The packet-witness instrument, which collapsed, and is reported anyway

The obvious way to connect the two halves is a call graph: for each tick
section, which clientbound packets can its code construct, and were any of them
ever emitted? It is built (`ci/jarcallgraph.py`), it runs, and it has almost no
resolving power — `invokevirtual` on `Entity`/`Level`/`BlockState` resolves to
every subtype, the server's tick graph is one giant strongly-connected
component, and a section as specific as `wardenBrain` comes out able to reach
51 of the 139 packet types whose class is resolvable. It calls 6 sections dark
where the two sharp rules call 41. It is printed under `--weak`, with its
collapse stated rather than deleted, because an instrument that could not
separate is a finding about the instrument and the next agent should not
rebuild it.

# A DENOMINATOR NOBODY CAN REACH MEASURES THE INSTRUMENT — 2026-08-12

Until this date every row above printed **two** numbers, `observed` and
`registry size`, and the second was silently doing the work of a third. It is
not the same quantity as *what the corpus could ever observe*, and on the row
where somebody went and measured, the two turned out to differ by more than
half: `block_entity_type` read `1 of 49`, and 29 of those 49 inherit
`BlockEntity.getUpdatePacket()`, which is `aconst_null; areturn`, so vanilla
never sends `block_entity_data` for them and the honest reading was `1 of 20`
(`docs/PARITY-ROADMAP.md` §25). The same shape held for sound events (§27) and
for menus (§28).

So every row now prints **three**: `observed / ceiling / registry`. The ceiling
is derived by the same standard as the denominator — Mojang's own `reports/`,
bytecode read through `ci/jarcallgraph.py` or `javap -p -c -constants` — and
never from `crates/`, which would measure what we chose to implement rather
than what vanilla can be made to say. The one exception is labelled `ours` and
carries its citation.

**Each ceiling carries an OWNER, and that is the part worth reading.** A
ceiling set by vanilla's own construction is a fact nobody can spend money on.
A ceiling set by *this harness* — a game rule `ci/t2-gameplay.sh` turns off, a
packet `trace::FAMILIES` makes ambient, a mob that cannot spawn in a peaceful
superflat — is work, and it is usually cheaper work than a scenario. The two
must not be added together and the script never does.

**A ceiling that cannot be derived is printed as `?`, not guessed.** An unknown
ceiling stated is worth more than a computed one that is wrong: it says the
registry size is an upper bound and nothing better is known, which is exactly
the position the whole file was in before this paragraph existed.

# A CEILING IS PER CHANNEL, AND THE OWNER IS A PROPERTY OF THE LIVE ONES — 2026-08-16

The paragraph above was right and the block-entity rule below it was not. That
rule read **one** route — `block_entity_data`, filtered by
`BlockEntity.getUpdatePacket` — and published its answer as the unit's ceiling
with an owner of `jar`. `getUpdatePacket` filters a route, not a type: the
chunk payload carries a `BlockEntityInfo` for every block entity in the chunk
and filters nothing, so vanilla's real ceiling on that unit is the whole
registry and the 29-type shortfall is `trace::FAMILIES` making ambient
`LEVEL_CHUNK_WITH_LIGHT` away. **`jar` told every reader not to spend money on
work that costs one decoder arm.** §29.7.

Two things follow, and they are the shape to copy for any future ceiling:

* **derive per channel and fold** (`live_ceiling`). A channel counts only if
  `trace.rs` neither drops its packet nor leaves it undecoded — a packet kept
  and never decoded is exactly as dark as one thrown away — and the owner is
  `jar` only when the live union equals the union over every channel vanilla
  has;
* **a rule that spends a recorder decision must READ that decision.**
  `ceiling_premise_housekeeping` refuses when the classification moves against
  `HOUSEKEEPING_PINNED`. Before it existed the premise lived in a docstring,
  both of the rule's guards read the jar rather than `trace.rs`, and deleting
  one line from `trace.rs` moved a published ceiling by 29 of 49 while every
  gate stayed green.

# The extraction can fail, and it says so rather than reporting a zero

Every set here is scraped or parsed, a scrape that stops matching returns the
empty set, and an empty set makes coverage look total. Each extraction asserts
a floor and this script exits 2 rather than print a number it cannot stand
behind. `--selftest` exercises the refusals.
"""

import argparse
import glob
import importlib.util
import json
import os
import re
import sys
from collections import Counter, defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
JAR = os.path.join(ROOT, "oracle-work", "vanilla-26.2", "versions", "26.2", "server-26.2.jar")
REPORTS = os.path.join(ROOT, "oracle-work", "vanilla-26.2", "generated", "reports")
T2 = os.path.join(ROOT, "ci", "t2-gameplay.sh")
CHECKS = os.path.join(ROOT, "ci", "checks", "gameplay")

TICK_ROOT = (
    "net/minecraft/server/MinecraftServer",
    "tickServer",
    "(Ljava/util/function/BooleanSupplier;)V",
)
PKT_PREFIX = "net/minecraft/network/protocol/game/Clientbound"
GAMERULES = "net/minecraft/world/level/gamerules/GameRules"

# Classes the ceiling rules read. All internal names in the pinned jar; every
# one of them is asserted present before it is used, because a class that has
# been renamed between versions otherwise turns into an empty set and an empty
# set reads as "nothing is reachable", which is the flattering direction.
BE_CLASS = "net/minecraft/world/level/block/entity/BlockEntity"
BE_TYPES = "net/minecraft/world/level/block/entity/BlockEntityTypes"
BE_IDS = "net/minecraft/world/level/block/entity/BlockEntityTypeIds"
BE_DATA_PACKET = "net/minecraft/network/protocol/game/ClientboundBlockEntityDataPacket"
UPDATE_PACKET = ("getUpdatePacket", "()Lnet/minecraft/network/protocol/Packet;")
CHUNK_DATA = "net/minecraft/network/protocol/game/ClientboundLevelChunkPacketData"
CHUNK_BE_INFO = CHUNK_DATA + "$BlockEntityInfo"
MENU_TYPE = "net/minecraft/world/inventory/MenuType"
BLOCK_CLASS = "net/minecraft/world/level/block/Block"

# A CEILING RULE THAT SPENDS A PACKET DROP MUST NAME THE DROP — 2026-08-16.
#
# **What a ceiling spends changed on 2026-09-06 and the pin got stronger, not
# weaker.** `trace::is_housekeeping` is gone: nothing is dropped at record time
# any more, and the classification is `trace::FAMILIES`, whose `Class::Ambient`
# rows are dropped by `compare::project` unless a scenario names the kind. So a
# ceiling can no longer spend "the recorder throws this away". It spends two
# weaker facts, both of which are still true and both of which are now checked:
# the family is **ambient by default** (no scenario in this corpus names it),
# and — the half that actually carries the block-entity ceiling — it is
# **undecoded**, so its payload is one opaque digest and no type is legible
# from it. `ceiling_premise_housekeeping` adjudicates both.
#
# Every entry here is a `trace::FAMILIES` id that some ceiling below
# depends on being ambient and undecoded. The dependency used to live in a docstring: the
# block-entity rule opened by *stating its own premise* — "`block_entity_data`
# is the census's only channel, because `trace::FAMILIES` makes ambient
# `LEVEL_CHUNK_WITH_LIGHT`" — and nothing anywhere read the list to check that
# the premise still held. Both of that rule's guards read
# `BlockEntity.getUpdatePacket`; neither read `trace.rs`. Un-drop the packet
# and the rule keeps returning 20, the ranked list keeps filtering on the
# registry gap, no test reddens, and §29 publishes a ceiling that is wrong by
# 29 types in the flattering direction.
#
# So the premise is data now, `ceiling_premise_housekeeping` adjudicates it,
# and the ceilings are recomputed per channel from the same list rather than
# asserted against it. The pin is what makes un-dropping a REVIEWED change:
# the number moves on its own, and the census refuses until whoever moved it
# has come back here and to `docs/PARITY-ROADMAP.md` §29.
# TWELVE UNTIL 2026-09-06, TEN SINCE, AND THE TWO THAT LEFT WERE MEASURED OUT.
#
# `SET_CHUNK_CACHE_CENTER` and `FORGET_LEVEL_CHUNK` are `Class::Graded` in
# `trace::FAMILIES` now. Both were measured **18 of 18 scenarios
# self-consistent across two independent vanilla runs, values AND the step each
# observation landed after** (17 and 187 observations per run respectively) —
# see `ci/checks/ambient-families.toml` and branch `recorder-housekeeping`.
# Neither is in `HOUSEKEEPING_SPENT_BY`, so no ceiling rule below is written on
# their being ambient; what moves is the protocol row's arithmetic, which is
# literally `141 - len(this list) - unbuildable`, and §29.2 records that.
#
# NINE ON MAIN: `LEVEL_CHUNK_WITH_LIGHT` had already left on branch
# `trace-chunk-block-entities` (its block-entity list is decoded), so the rebase
# onto main 3230 takes the two measured out here from eleven, not twelve.
HOUSEKEEPING_PINNED = {
    "KEEP_ALIVE", "PING", "SET_TIME",
    "CHUNK_BATCH_START", "CHUNK_BATCH_FINISHED",
    "LIGHT_UPDATE",
    "PLAYER_INFO_UPDATE", "TAB_LIST", "UPDATE_ADVANCEMENTS",
}

# Which pinned drops a ceiling actually spends, and what it costs to un-drop
# one. Keyed by packet, valued by the rows whose ceiling moves.
#
# LEVEL_CHUNK_WITH_LIGHT left this dict on trace-chunk-block-entities: it is no
# longer dropped by `is_housekeeping` (the chunk block-entity list is decoded
# now) and the chunk channel is live, so `live_ceiling` folds it into the union
# on its own and the row's owner is `jar` at 49 — see
# docs/PARITY-ROADMAP.md §29.7/§29.8. Nothing is spent here any more; a packet
# only belongs in this dict while some ceiling rule still depends on it staying
# dropped.
HOUSEKEEPING_SPENT_BY = {}

# The sound ceiling is CITED, not derived, and this is the whole of the reason
# it is allowed to be. Every other ceiling in this file is a property of the
# jar; this one is a property of OUR EMITTER, measured by another branch, and a
# census that went and read `crates/` for it would stop being a census of
# vanilla. It is carried as a citation with an owner of `ours` so that it reads
# as what it is: a ceiling this project can move, and the only number here that
# goes stale when our own source changes.
CITED_SOUND_CEILING = {
    "value": 256,
    "owner": "ours",
    "cite": "ROADMAP §27.1, branch `gameplay-sound-coverage`, 2026-08-12",
    "why": "122 *.place + 114 *.step + 21 hand-written names, union 256, from "
           "crates/rustmc-server/src/sounds.rs and world_send::place_sound — the only "
           "emitters. A scenario listening for anything else reports `absent` and "
           "adjudicates nothing. RE-DERIVE THIS WHEN THAT FILE CHANGES.",
}

# Sections vanilla's own gate turns off under a game rule the harness sets.
#
# ASSERTED HERE, VERIFIED BELOW. Each pair is checked against the jar in two
# ways before it is allowed to count: the rule id must exist in Mojang's
# `minecraft:game_rule` registry (26.2 renamed every one of these and a stale
# id has silently cost this project a whole corpus once — see the AGENT-BRIEF),
# and the section's own code, within `HOPS` static call hops up or down from the
# method that pushes it, must read `GameRules.<RULE>`. A pair that fails either
# check is reported as UNVERIFIED and is not counted. `thunder` is such a pair
# and is left in deliberately: its real gate is the weather *state*, which
# `advance_weather=false` freezes, not the rule field, so it is the standing
# positive control that this verifier can refuse.
RULE_SECTIONS = {
    "advance_weather": ["weather", "thunder"],
    "random_tick_speed": ["randomTick"],
    "spawn_mobs": [
        "naturalSpawnCount",
        "filteringSpawningChunks",
        "shuffleSpawningChunks",
        "tickSpawningChunks",
        "spawner",
        "customSpawners",
    ],
}

# Section-name suffixes that make a section per-mob. Mojang's convention, not
# ours: `wardenBrain` and `frogActivityUpdate` are pushed only while that mob
# ticks. The stripped stem is resolved against the `minecraft:entity_type`
# registry and a stem that does not resolve leaves its section unclassified.
MOB_SUFFIX = re.compile(r"^(.*?)(Brain|ActivityUpdate)$")

# How far the gate verifier walks from the pushing method, in each direction.
# Three is the smallest number that reaches `ServerChunkCache.tickChunks` from
# `ServerLevel.tickChunk`, which is two hops because the rule value crosses a
# captured lambda parameter (`lambda$tickChunks$0`) on the way down.
HOPS = 3


def die(msg):
    print(f"vanilla-reach-census: REFUSED — {msg}", file=sys.stderr)
    sys.exit(2)


def sha(path):
    import hashlib  # noqa: PLC0415

    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def stamp(jar, files, ncheck):
    """The line every figure taken from this census pins itself to.

    Four inputs, all four in the stamp, because this ledger has already
    published a figure that survived a re-measure by pinning only one of its
    two inputs (`corpus-floor-n6-coverage92`, `ci/figures.toml`). A census of
    what the corpus reaches moves when the jar moves, when the recordings move,
    when the corpus grows, AND when this script's own definitions change — so a
    claim string that omits any of them can be inherited by a measurement that
    is not the same measurement.
    """
    import hashlib  # noqa: PLC0415

    rec = hashlib.sha256("".join(sorted(sha(f) for f in files)).encode()).hexdigest()
    me = hashlib.sha256(
        open(os.path.abspath(__file__), "rb").read()
        + open(os.path.join(ROOT, "ci", "jarcallgraph.py"), "rb").read()
    ).hexdigest()
    return f"jar={sha(jar)[:12]} recordings={rec[:12]}(n={len(files)}) census={me[:12]} corpus={ncheck}"


def camel_to_snake(s):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", s).lower()


# --------------------------------------------------------------------------
# the jar's own enumerations
# --------------------------------------------------------------------------


def registries(path=None):
    p = path or os.path.join(REPORTS, "registries.json")
    if not os.path.exists(p):
        die(f"{p} not found — link oracle-work in (see docs/AGENT-BRIEF.md)")
    d = json.load(open(p))
    if len(d) < 50:
        die(f"only {len(d)} registries in registries.json — wrong or truncated report")
    return d


def reg_names(regs, key):
    if key not in regs:
        die(f"registry {key} is absent from the pinned jar's registries.json")
    return set(regs[key]["entries"]), {v["protocol_id"]: k for k, v in regs[key]["entries"].items()}


def tick_sections(graph):
    """Mojang's own names for what a server tick does, and where each is pushed.

    Returns `(sections, pushers, calls, reachable)`:
      * `sections[name]` -> number of push/popPush sites
      * `pushers[name]`  -> set of methods containing such a site
      * `calls[name]`    -> set of unresolved call targets lexically inside it
      * `reachable`      -> every method reachable from `MinecraftServer.tickServer`

    A section is entered by `push` or `popPush` and left by `pop` or the next
    `popPush`; calls are attributed to the innermost section open at their
    bytecode offset. That is a linear reading of a possibly-branching method,
    and it is the approximation this unit rests on: a call in a branch that
    re-enters a section after a `pop` would be mis-attributed. It bounds the
    `calls` set only — the section *names*, which are the denominator, are read
    off the `push` operands and are exact.
    """
    if TICK_ROOT not in graph.calls:
        die(f"{TICK_ROOT[0]}.{TICK_ROOT[1]} is not in the pinned jar — the tick entry point moved")
    reachable = graph.reach([TICK_ROOT])
    sections, pushers, calls = Counter(), defaultdict(set), defaultdict(set)
    nameless = 0
    for m in reachable:
        stack = []
        for _off, kind, payload in graph.events.get(m, ()):
            if kind in ("push", "popPush"):
                if kind == "popPush" and stack:
                    stack.pop()
                if payload is None:
                    nameless += 1
                stack.append(payload)
                if payload is not None:
                    sections[payload] += 1
                    pushers[payload].add(m)
            elif kind == "pop":
                if stack:
                    stack.pop()
            elif kind == "call" and stack and stack[-1] is not None:
                calls[stack[-1]].add(payload)
    if len(sections) < 80:
        die(f"only {len(sections)} profiler sections found under the tick — extraction is broken")
    return sections, pushers, calls, reachable, nameless


def rule_is_gated(graph, pushers, section, rule, callers):
    """Does `section`'s own code read `GameRules.<RULE>`, within two static hops
    down or one hop up?

    The upward hop is not slack. `ServerChunkCache.tickChunks` reads
    `RANDOM_TICK_SPEED` into a local and passes it to `ServerLevel.tickChunk`,
    which is the method that pushes `randomTick`; a downward-only search calls
    the single most important gate in this census unverified. One hop up, and
    only to callers that are themselves inside the tick graph, is the smallest
    extension that reaches a rule read into a parameter.

    Deliberately *not* the devirtualised graph. This is a verification, so it
    has to be able to fail: over the resolved graph every section reaches
    everything and the check would pass by construction, which is the shape of
    gate this project keeps finding and calling a gate. It does fail — see the
    `thunder` row, which is the standing positive control, and the negative
    control in `main` which refuses `advance_weather -> randomTick`.

    **This is a staleness guard, not a proof.** It cannot separate two rules
    read in the same method: `ServerChunkCache.tickChunks` reads both
    `SPAWN_MOBS` and `RANDOM_TICK_SPEED`, so either would verify for either of
    the sections below it. What it does catch is the two failures that have
    actually cost this project time — a game rule renamed between versions and
    a profiler section that has moved or gone — and it catches them on the day
    the jar is bumped rather than in a figure nobody re-derived.
    """
    want_owner, want_name = GAMERULES, rule.upper()

    def reads(m):
        return any(f[0] == want_owner and f[1] == want_name for f in graph.fields.get(m, ()))

    def walk(start, step):
        frontier, seen = set(start), set()
        for _hop in range(HOPS):
            if any(reads(m) for m in frontier):
                return True
            seen |= frontier
            frontier = set().union(*(step(m) for m in frontier)) - seen if frontier else set()
        return any(reads(m) for m in frontier)

    down = lambda m: {  # noqa: E731
        (t[0], t[1], t[2]) for t in graph.calls.get(m, ()) if (t[1], t[2]) in graph.declares.get(t[0], ())
    }
    up = lambda m: set(callers.get(m, ()))  # noqa: E731
    start = set(pushers.get(section, ()))
    return walk(start, down) or walk(start, up)


# --------------------------------------------------------------------------
# the ceilings — what the corpus could ever observe, read out of the same jar
#
# Each function below answers one question and states which one, because the
# rule differs per unit and a single "reachable" predicate over five registries
# would be five different claims wearing one name.
# --------------------------------------------------------------------------


def javap(jar, cls):
    """`javap -p -c -constants` on one class of the pinned jar, as text."""
    import subprocess  # noqa: PLC0415
    import tempfile  # noqa: PLC0415
    import zipfile  # noqa: PLC0415

    # Refuses on a missing jar rather than raising, so that this is one of the
    # guards `--selftest` can fire in a tree with no `oracle-work` linked in —
    # which is every tree `ci/t0.sh` runs in that has not been handed off to.
    if not os.path.exists(jar):
        die(f"{jar} not found — link oracle-work in (see docs/AGENT-BRIEF.md)")
    with tempfile.TemporaryDirectory() as td:
        with zipfile.ZipFile(jar) as z:
            try:
                z.extract(cls + ".class", td)
            except KeyError:
                die(f"{cls} is not in the pinned jar — a ceiling rule's anchor class moved")
        out = subprocess.run(
            ["javap", "-p", "-c", "-constants", os.path.join(td, cls + ".class")],
            capture_output=True, text=True, check=False,
        )
    if not out.stdout:
        die(f"javap produced nothing for {cls}: {out.stderr.strip()[:200]}")
    return out.stdout


def chain(graph, cls):
    """`cls` and every supertype above it, nearest first."""
    seen, out, stack = set(), [], [cls]
    while stack:
        c = stack.pop(0)
        if c in seen:
            continue
        seen.add(c)
        out.append(c)
        stack.extend(graph.supers.get(c, ()))
    return out


def ceiling_premise_housekeeping(keep, spent=None, pinned=None, undecoded=None):
    """THE GUARD. Refuses when `trace::FAMILIES` stops matching its pin.

    A ceiling that is only true because the recorder throws a packet away is a
    ceiling with a premise in another file, and this project's central defect
    is a check whose green cannot be turned red. This one could not: the
    block-entity rule *named* `trace::is_housekeeping` dropping
    `LEVEL_CHUNK_WITH_LIGHT` in its own first paragraph, and neither of its two
    guards, nor the ranked list, nor any test in `ci/t0.sh` ever opened
    `trace.rs`. Deleting that one line from `is_housekeeping` moved a published
    ceiling by 29 of 49 types and reddened nothing.

    `keep` is the ambient id-name set as read out of `trace.rs` by
    `ci/corpus-surface.py`'s `trace_sets`; `undecoded` is the subset for which
    `trace::decode` has no arm, from `undecoded_families`. Three refusals:

    * **a spent drop is gone.** A packet some ceiling rule below depends on
      being dropped is no longer dropped, so that ceiling has already changed.
      Named individually, with what it costs, because this is the case that
      actually moves a number;
    * **the list moved at all.** Anything added or removed. The protocol row's
      ceiling is literally `141 - len(housekeeping) - unbuildable`, and §29
      publishes both that number and the twelve by name, so any edit here
      restates a published figure. The census recomputes; the pin is what makes
      somebody come and say so.

    * **a spent drop has acquired a decoder arm.** Added 2026-09-06 with the
      classification table. The block-entity rule's real premise was never
      "the packet is unrecorded" — it is "no block-entity type is legible from
      the chunk payload", and what makes that true is the absence of an arm.
      Recording `LEVEL_CHUNK_WITH_LIGHT`, which this tree now does, moves the
      ceiling by nothing; writing its arm moves it by 29 of 49 types. The
      earlier version of this guard would have passed the second while
      refusing the first, which is exactly backwards.

    Refusing on the *whole* list rather than only on the spent entries is
    deliberate and is the ratchet idiom this repo already applies to the tier-2
    floor and the dead-field ceiling: widening is allowed, silently widening is
    not.
    """
    pinned = HOUSEKEEPING_PINNED if pinned is None else pinned
    spent = HOUSEKEEPING_SPENT_BY if spent is None else spent
    if undecoded is not None:
        armed = sorted(p for p in spent if p not in undecoded)
        if armed:
            detail = "; ".join(f"{p}: {spent[p]}" for p in armed)
            die(
                f"trace::decode now has an arm for {', '.join(armed)}, and a ceiling "
                f"rule here is written on the premise that it does not — {detail}. "
                "Re-derive that row's ceiling and update docs/PARITY-ROADMAP.md §29 "
                "and the matching ci/figures.toml entry before this census is quotable"
            )
    lost = sorted(p for p in spent if p not in keep)
    if lost:
        detail = "; ".join(f"{p}: {spent[p]}" for p in lost)
        die(
            f"trace::FAMILIES no longer makes {', '.join(lost)} ambient, and a ceiling rule "
            f"here is written on the premise that it does — {detail}. Re-derive that row's "
            "ceiling, move its owner, and update docs/PARITY-ROADMAP.md §29 and the "
            "matching ci/figures.toml entry before this census is quotable again"
        )
    added, removed = sorted(keep - pinned), sorted(pinned - keep)
    if added or removed:
        die(
            "trace::FAMILIES' ambient set has moved against HOUSEKEEPING_PINNED"
            + (f" — added {', '.join(added)}" if added else "")
            + (f" — removed {', '.join(removed)}" if removed else "")
            + ". Every ceiling that spends a drop is derived from this list, and §29 "
            "publishes its size and its members by name. Update HOUSEKEEPING_PINNED in "
            "the same commit that moves the list, and re-take the §29 artefact"
        )
    return True


def chunk_payload_carries_every_block_entity(jar, cls=None, info=None, filter_cls=None):
    """Does the chunk payload filter block entities the way `block_entity_data` does?

    The second channel's whole claim is that it does **not**. Read off
    `ClientboundLevelChunkPacketData.<init>(LevelChunk)`: it walks
    `LevelChunk.getBlockEntities().entrySet()` and hands each value to
    `BlockEntityInfo.create`, which reads `getUpdateTag`, `getBlockPos` and
    `getType` — and never `getUpdatePacket`. So every block entity present in a
    delivered chunk is on the wire regardless of what its class overrides, and
    this channel's reachable set is the whole registry.

    Returns `True`, or refuses. The refusal is the point: if a future version
    filters this list the way the packet channel does, "the whole registry" is
    no longer the answer and the census must not go on printing it.
    """
    cls = CHUNK_DATA if cls is None else cls
    info = CHUNK_BE_INFO if info is None else info
    # `filter_cls` is the class whose bytecode must NOT read `getUpdatePacket`.
    # It is a parameter only so that `--selftest` can point the filter arm at a
    # class that does read it and watch this refuse: a guard nobody has seen
    # fire is not a guard.
    filter_cls = info if filter_cls is None else filter_cls
    text = javap(jar, cls)
    m = re.search(
        r"public " + re.escape(cls.replace("/", "."))
        + r"\(net\.minecraft\.world\.level\.chunk\.LevelChunk\);(.*?)(?=\n  \S|\Z)",
        text, re.S,
    )
    if not m:
        die(f"{cls} has no (LevelChunk) constructor in the pinned jar — the chunk channel's anchor moved")
    body = m.group(1)
    if "LevelChunk.getBlockEntities" not in body:
        die(f"{cls}.<init> no longer reads LevelChunk.getBlockEntities — the chunk channel is not derivable")
    if f"{info}.create" not in body:
        die(f"{cls}.<init> no longer calls {info.rsplit('$', 1)[-1]}.create — the chunk channel is not derivable")
    if "getUpdatePacket" in body or "getUpdatePacket" in javap(jar, filter_cls):
        die(
            f"{filter_cls} now reads getUpdatePacket — the chunk payload has grown the same "
            "filter as block_entity_data, so its reachable set is no longer the whole registry "
            "and this channel's ceiling must be re-derived"
        )
    return True


def live_ceiling(channels, housekeeping, decoded):
    """Fold per-channel reachable sets into the one number a row may publish.

    **This is where a ceiling acquires an owner**, and it is a separate
    function so that the fold can be shown moving. Given
    `{channel: (packet, ids)}`:

    * a channel is LIVE only if `trace.rs` neither drops its packet in
      `trace::FAMILIES` nor leaves it undecoded. Both are recorder decisions.
      A packet that is kept but never decoded is exactly as dark as one that is
      thrown away, and reading only the drop list would score it as live;
    * the ceiling is the union over the live channels — what this census could
      ever observe;
    * `vanilla` is the union over *every* channel — what vanilla can be made to
      say at all;
    * the owner is `jar` only when those two are equal. Any gap between them is
      a channel we chose not to read, which is work, and calling it `jar` tells
      a reader not to spend money on something one decoder arm would buy.

    Returns `(reach, vanilla, owner, live_channels, dark_channels)`.
    """
    live = {
        name: (pkt, ids) for name, (pkt, ids) in channels.items()
        if pkt not in housekeeping and pkt in decoded
    }
    reach = set().union(*(ids for _p, ids in live.values())) if live else set()
    vanilla = set().union(*(ids for _p, ids in channels.values())) if channels else set()
    dark = sorted((name, pkt) for name, (pkt, _i) in channels.items() if name not in live)
    return reach, vanilla, ("jar" if reach == vanilla else "ours"), live, dark


def ceiling_block_entities(graph, jar, all_ids):
    """RULE, PER CHANNEL: which block entity types can each route ever name?

    **There are two channels and the ceiling is a property of the live ones**,
    which is the correction of 2026-08-16. This function used to answer for one
    channel and present it as the unit's ceiling, with the other channel's
    absence asserted in prose (see `ceiling_premise_housekeeping`).

    * `block_entity_data` — one emit site, `ChunkHolder.broadcastBlockEntity`,
      which calls `BlockEntity.getUpdatePacket()` and returns on null. The base
      method is `aconst_null; areturn`, so a type that does not override it is
      never the subject of one. `docs/PARITY-ROADMAP.md` §25, measured under a
      rig with four inheriting controls that placed their block and said
      nothing;
    * `chunk_payload` — `LEVEL_CHUNK_WITH_LIGHT`, whose
      `ClientboundLevelChunkPacketData` carries a `BlockEntityInfo` for **every**
      block entity in the chunk with no `getUpdatePacket` filter at all, so its
      reachable set is the whole registry. Guarded by
      `chunk_payload_carries_every_block_entity`.

    Returns `{channel: (packet_id_name, reachable_ids)}`, `minecraft:`-prefixed.
    The caller intersects that with which packets the recorder actually keeps
    and decodes; a channel's ceiling is not a ceiling the census can reach if
    `trace.rs` throws the packet away.

    Two guards on the packet channel, because that rule is one method body wide
    and a version bump could quietly make it vacuous:

    * the base body must still be exactly `aconst_null; areturn`. If Mojang
      ever gives `BlockEntity` a real default, *every* type overrides nothing
      and this function would call all 49 reachable while meaning the opposite;
    * every override found must construct a `ClientboundBlockEntityDataPacket`.
      An override that returns null down some path is not caught by this — it
      is a bound, and it is the conservative direction for a ceiling.
    """
    base = javap(jar, BE_CLASS)
    m = re.search(r"getUpdatePacket\(\);\s*\n\s*Code:\s*\n((?:\s+\d+: \w+.*\n)+)", base)
    if not m:
        die(f"{BE_CLASS}.getUpdatePacket has no body in the pinned jar — the rule's anchor moved")
    ops = [ln.split(":", 1)[1].split()[0] for ln in m.group(1).strip().splitlines()]
    if ops != ["aconst_null", "areturn"]:
        die(
            f"{BE_CLASS}.getUpdatePacket is {ops} and not ['aconst_null', 'areturn'] — "
            "the base no longer returns null, so 'does not override' no longer means "
            "'sends nothing' and this ceiling rule is void"
        )

    # Pair each `getstatic BlockEntityTypeIds.<NAME>` with the constructor
    # invoked after it and before the next one. Both live in the same method,
    # so this is an offset ordering inside one linear `<clinit>` and not a
    # positional guess across two class files.
    text = javap(jar, BE_TYPES)
    ids = [
        (int(g1), g2)
        for g1, g2 in re.findall(
            rf"^\s*(\d+): getstatic\s+#\d+\s+// Field {BE_IDS}\.([A-Z0-9_]+):", text, re.M
        )
    ]
    if len(ids) < 40:
        die(f"only {len(ids)} BlockEntityTypeIds reads in {BE_TYPES}.<clinit> — the scrape is broken")
    ev = graph.events.get((BE_TYPES, "<clinit>", "()V"))
    if not ev:
        die(f"{BE_TYPES}.<clinit> carries no events — block entity registration moved")
    ctors = sorted((o, t[0]) for o, k, t in ev if k == "call" and t[1] == "<init>")

    reach, blocked, unresolved = set(), set(), []
    for i, (off, field) in enumerate(ids):
        nxt = ids[i + 1][0] if i + 1 < len(ids) else 1 << 30
        cand = [c for o, c in ctors if off < o < nxt]
        rid = "minecraft:" + field.lower()
        if not cand:
            unresolved.append(field)
            continue
        cls = cand[0]
        over = [c for c in chain(graph, cls) if c != BE_CLASS and UPDATE_PACKET in graph.declares.get(c, ())]
        if over:
            # Eighteen of the nineteen overriding classes declare `getUpdatePacket`
            # TWICE: the covariant method returning `ClientboundBlockEntityDataPacket`
            # and the synthetic bridge returning `Packet`, which is the one that
            # matches the base's descriptor and whose whole body is a call to the
            # other. Checking only the bridge sees a call to itself and no packet —
            # this guard fired on all eighteen when it was written that way, which
            # is the guard working and the reading of it being wrong. Union every
            # `getUpdatePacket` the class declares.
            bodies = [(over[0], n, d) for n, d in graph.declares.get(over[0], ()) if n == "getUpdatePacket"]
            builds = any(
                t[0] == BE_DATA_PACKET for b in bodies for t in graph.calls.get(b, ())
            ) or any(BE_DATA_PACKET in graph.news.get(b, ()) for b in bodies)
            if not builds:
                die(
                    f"{over[0]}.getUpdatePacket overrides the base but constructs no "
                    f"{BE_DATA_PACKET} — the corroborating guard failed and the ceiling "
                    "cannot be stood behind"
                )
            reach.add(rid)
        else:
            blocked.add(rid)
    if unresolved:
        die(f"{len(unresolved)} block entity types resolved to no constructor: {unresolved[:6]}")

    # The second channel. `all_ids` is the registry, and the guard is what
    # entitles this line to be the whole of it rather than a subset — without
    # it, "no filter" would be an assertion in a docstring, which is exactly
    # the shape this whole change exists to remove.
    chunk_payload_carries_every_block_entity(jar)
    if not reach <= all_ids or not blocked <= all_ids:
        die("a block entity type resolved from BlockEntityTypes.<clinit> is not in "
            "reports/registries.json — the two enumerations have diverged")
    return {
        "block_entity_data": ("BLOCK_ENTITY_DATA", reach),
        "chunk_payload": ("LEVEL_CHUNK_WITH_LIGHT", set(all_ids)),
    }


def ceiling_menus(graph):
    """RULE: is the menu's own factory called from anywhere but `MenuType`?

    A `MenuType` is registered with the factory that builds its menu, and the
    only way a client ever sees one is a server calling that factory. Four of
    the twenty-five — `generic_9x1`, `_9x2`, `_9x4`, `_9x5` — are named by
    exactly one class in the whole jar, the registration site itself. No block,
    block entity, entity or command opens any of them. The control is in the
    same scan and not in a separate argument: `ChestMenu.threeRows`, the same
    shape of factory on the same class, has five callers.
    `docs/PARITY-ROADMAP.md` §28.

    Returns `{id: (verdict, callers)}` with verdict in
    `block` / `entity-only` / `no-opener`. The split matters because the two
    non-`block` verdicts have different owners: `no-opener` is vanilla's, and
    `entity-only` is this harness's — `merchant` needs a villager, and
    `ci/t2-gameplay.sh` boots `spawn_mobs=false`, `spawn-monsters=false` and
    `difficulty=peaceful`.

    Matched on `(owner, name)` and not on the descriptor. The registration site
    uses the two-argument `(int, Inventory)` overload every time and every real
    opener uses a wider one; a descriptor-exact match scores all twenty-five
    unreachable, which is how this rule was first written and how it was caught.
    """
    ev = graph.events.get((MENU_TYPE, "<clinit>", "()V"))
    if not ev:
        die(f"{MENU_TYPE}.<clinit> carries no events — menu registration moved")
    fac, cur = {}, None
    for _off, kind, p in ev:
        if kind == "str":
            cur = p
        elif kind == "call" and cur and not (p[0] == MENU_TYPE and p[1] == "register"):
            if p[0] == MENU_TYPE:
                # A synthetic lambda in the registration class itself — `lectern`
                # is the only one in 26.2. Hop once, to what it constructs.
                tgt = [t for t in graph.calls.get((MENU_TYPE, p[1], p[2]), ()) if t[1] == "<init>"]
                if tgt:
                    fac.setdefault(cur, sorted(tgt)[0])
            else:
                fac.setdefault(cur, p)
    if len(fac) < 20:
        die(f"only {len(fac)} menu factories resolved out of {MENU_TYPE}.<clinit> — the scrape is broken")

    callers = defaultdict(set)
    for m, ts in graph.calls.items():
        for t in ts:
            callers[(t[0], t[1])].add(m[0])
    sub = graph.subtypes()
    blockish = sub.get(BLOCK_CLASS, set()) | {BLOCK_CLASS} | sub.get(BE_CLASS, set()) | {BE_CLASS}

    out = {}
    for mid, f in fac.items():
        cs = sorted(callers.get((f[0], f[1]), set()) - {MENU_TYPE, f[0]})
        # `ChestBlock$2$1` is the two-half combiner; strip the anonymous-class
        # suffix before asking whether the caller is block-side.
        blk = [c for c in cs if c.split("$")[0] in blockish]
        out["minecraft:" + mid] = (("block" if blk else "entity-only") if cs else "no-opener", cs)
    return out


def ceiling_packets(graph, cls_of):
    """RULE: is the packet class constructed anywhere in the pinned jar?

    A clientbound type an unmodified server never builds cannot be emitted by
    one, whatever the corpus does. **The rule fires on nothing** — all 139
    types whose class `GameProtocols` resolves are constructed somewhere, by
    `new` or by a `::new` method reference reaching the codec — and that is
    the result, not a failure: vanilla sets no ceiling on this unit below the
    registered 141, so every reduction printed for it is one of ours.

    Returns the set of type names the rule excludes, which is expected to be
    empty and is measured rather than asserted.

    A `new` scan alone is wrong here and was the first version: records built
    only through `Clientbound…::new` handed to a `StreamCodec` never carry the
    `new` opcode at the call site, and that version called ten types dead,
    `SET_TITLE_TEXT` among them, which `/title` plainly sends. The `<init>`
    call targets are the other half.
    """
    built = set()
    for cs in graph.news.values():
        built |= cs
    for ts in graph.calls.values():
        for t in ts:
            if t[1] == "<init>":
                built.add(t[0])
    return {n for n, c in cls_of.items() if c and c not in built}


def spawn_egg_types(regs):
    """A FLOOR ON the entity-type ceiling, and deliberately not the ceiling.

    There is no honest jar rule that caps this unit: with a creative hotbar,
    blocks, items and a world, most of the 158 could in principle be made to
    appear, and enumerating the ones that could not would be invention. What
    *is* readable off Mojang's own item registry is which entity types have a
    spawn egg — those a scenario reaches with one hotbar slot and no new verb.

    The convention `<entity type>_spawn_egg` is checked rather than assumed:
    every stem must resolve to a registered entity type or this refuses.
    """
    items, _ = reg_names(regs, "minecraft:item")
    ents, _ = reg_names(regs, "minecraft:entity_type")
    eggs = {i[: -len("_spawn_egg")] for i in items if i.endswith("_spawn_egg")}
    bad = sorted(eggs - ents)
    if bad:
        die(f"{len(bad)} spawn eggs whose stem is not an entity type: {bad[:5]} — the convention broke")
    if len(eggs) < 50:
        die(f"only {len(eggs)} spawn eggs in the item registry — the scrape is broken")
    return eggs


# --------------------------------------------------------------------------
# the corpus's own numerators — measured off recordings, never read off source
# --------------------------------------------------------------------------


def recordings(pattern):
    files = sorted(glob.glob(os.path.expanduser(pattern)))
    if not files:
        die(f"no recordings matched {pattern} — this unit is a measurement and needs them")
    return files


def scenario_vocabulary(regs, checks=None):
    """The set of item ids any scenario in the corpus can put in front of vanilla.

    **This is NOT a recording, and the item row must never let it be read as
    one.** It is a static property of `ci/checks/gameplay/*.toml`, in the same
    class as the protocol unit's `checks` column, and it is here because
    without it the item row's denominator is actively misleading.

    An item reaches a recording only if some scenario puts it somewhere
    vanilla will talk about, and the only verb that puts an item into a
    harness player's inventory is `hold` — `set_creative_mode_slot`.
    `Step::Command` cannot run `/give`, which is permission level 2, and
    `ci/t2-gameplay.sh`'s players are not operators. So the items named by
    `hold` steps are a **hard ceiling** on the numerator: no recording of this
    corpus can reach an item outside this set, however many times it is run
    and however long it runs for.

    That makes `12 / 1537` two numbers with different meanings glued together.
    1,537 is the registry; the corpus can reach 56 of it; and what the
    recordings actually add is 12 of those 56. Quoting the first ratio as
    coverage says the corpus is 0.78% of the way through a queue. It is not
    working through a queue at all — it is measuring a vocabulary somebody
    chose, and the only thing that moves it is a new scenario naming a new
    item.

    Items named in a scenario but absent from the registry are a scenario bug
    and are named rather than silently dropped: `Step::Hold` would refuse at
    run time with "no item called X in the extract", which arrives mid-check
    and reads like a server failure.
    """
    import tomllib

    names, _ = reg_names(regs, "minecraft:item")
    paths = sorted(glob.glob(os.path.join(checks or CHECKS, "*.toml")))
    if not paths:
        die(f"no scenarios at {os.path.relpath(checks or CHECKS, ROOT)}")
    held, unknown = set(), set()
    for p in paths:
        with open(p, "rb") as f:
            doc = tomllib.load(f)
        for phase in ("setup", "step"):
            for s in doc.get(phase, ()):
                if s.get("do") != "hold":
                    continue
                it = s.get("item")
                if it is None:
                    continue
                (held if it in names else unknown).add(it)
    if unknown:
        die(
            "scenarios name items the registry does not hold, which `Step::Hold` "
            f"would refuse mid-check: {', '.join(sorted(unknown))}"
        )
    return held


def registry_reach(files, regs):
    """What the recordings prove the corpus's worlds ever contained.

    Every reading is an *observation of vanilla's own output*, so absence is
    evidence: `add_entity` is recorded for every entity that enters view, so a
    mob type that never appears in six runs was never there.

    **That sentence is true of every bucket here except `block` and
    `block_state`, and the exception is load-bearing.** Those two are read out
    of `block_update`, `section_blocks_update` and `block_event` — packets
    vanilla sends when a block *changes*. A block that is merely present is
    never mentioned. The proof is in the corpus's own substrate: vanilla's
    superflat is bedrock, two layers of dirt and grass on top, and of those
    four `grass_block` scores because things get placed on it while **`dirt`
    and `bedrock` score zero in all six recordings**. So read these two rows
    as *blocks the corpus caused vanilla to broadcast a change of*, which is a
    strictly smaller set than what the world contained, and a set our own
    scenarios choose. `block` is directly sweepable — a scenario that holds an
    item, places it and asserts nothing adds one — and a sweep would move it
    to about 1,054, the number of items in the pinned jar that place a block,
    without measuring one additional mechanism. Say which you mean before
    quoting either.

    Items were absent until 2026-08-12 and are not any more. `trace::observe`
    digested every `ItemStack` into `rest`, so this census could not reach the
    item registry at all and a zero there would have been a reading of the
    recorder rather than of vanilla. Branch `trace-itemstack-legible` decodes
    the stack, and the numerator below is now a measurement — with one
    remaining blind spot named in `item_opaque`: a stack whose data-component
    patch adds a component the decoder has no codec for is read down to its
    item id and no further, which does not hide the item (the id precedes the
    patch) but does stop the enclosing packet, so later stacks in the same
    packet go unread. That count is printed rather than folded in.

    **A recording made before that branch has no item fields at all, and this
    function must not answer zero for one.** `item_bearing` counts the
    observations that carry a stack by construction; if the corpus produced
    them and not one yielded an item, the recordings pre-date the decoder and
    the caller says so instead of printing a percentage.
    """
    _ent_names, ent_by_id = reg_names(regs, "minecraft:entity_type")
    _snd_names, snd_by_id = reg_names(regs, "minecraft:sound_event")
    _be_names, be_by_id = reg_names(regs, "minecraft:block_entity_type")
    _menu_names, menu_by_id = reg_names(regs, "minecraft:menu")
    _blk_names, blk_by_id = reg_names(regs, "minecraft:block")

    seen = defaultdict(set)
    unknown = Counter()
    tally = Counter()

    def note(bucket, table, key):
        if key in table:
            seen[bucket].add(table[key])
        else:
            unknown[bucket] += 1

    # Every kind whose decoder arm can put a stack in its fields. Kept as a
    # set rather than folded into the walk so that `item_bearing` counts what
    # SHOULD have carried an item, which is what makes an empty numerator
    # distinguishable from an old recording.
    stack_kinds = {
        "container_set_slot",
        "container_set_content",
        "set_cursor_item",
        "set_player_inventory",
        "set_equipment",
    }

    def walk_stacks(node):
        """Every decoded `ItemStack` at any depth, by shape and not by path.

        `read_item_stack` writes `{count, item_id, item, ...}`, and a stack can
        sit directly under `item`, inside a `slots` array, or under `carried`.
        Matching on the shape means a decoder arm added later is censused
        without this script being edited — the same reason `trace::ENTITY_REF`
        is a marker rather than a field list.
        """
        if isinstance(node, dict):
            if "item_id" in node and "item" in node:
                seen["item"].add(node["item"])
                if "opaque_component" in node:
                    tally["item_opaque"] += 1
                yield node
            for v in node.values():
                yield from walk_stacks(v)
        elif isinstance(node, list):
            for v in node:
                yield from walk_stacks(v)

    scen = set()
    for path in files:
        d = json.load(open(path))
        for s in d.get("scenarios", []):
            scen.add(s.get("name", "?"))
            for o in s.get("obs", ()):
                k, f = o.get("kind"), o.get("fields", {})
                if k in stack_kinds:
                    tally["item_bearing"] += 1
                    for _stack in walk_stacks(f):
                        tally["item_stacks"] += 1
                if k == "add_entity":
                    note("entity_type", ent_by_id, f.get("kind_id"))
                elif k == "sound":
                    note("sound_event", snd_by_id, f.get("sound"))
                elif k == "block_entity_data":
                    note("block_entity_type", be_by_id, f.get("block_entity_type"))
                elif k == "chunk_block_entities":
                    # The SECOND channel this unit has, and the one that is
                    # not restricted to the 20 types that override
                    # `BlockEntity.getUpdatePacket()`. Vanilla puts every block
                    # entity in a chunk into that chunk's payload with no
                    # filter, so a type reaching a client here is the same
                    # kind of evidence as one reaching it on
                    # `block_entity_data`: an observation of vanilla's own
                    # output, into the same `seen` bucket.
                    for b in f.get("block_entities", ()):
                        note("block_entity_type", be_by_id, b.get("block_entity_type"))
                elif k == "open_screen":
                    note("menu", menu_by_id, f.get("menu"))
                elif k == "block_event":
                    note("block", blk_by_id, f.get("block"))
                elif k == "block_update":
                    seen["block"].add(f.get("state", "").split("[")[0])
                    seen["block_state"].add(f.get("state_id"))
                elif k == "section_blocks_update":
                    for b in f.get("blocks", ()):
                        seen["block"].add(b.get("state", "").split("[")[0])
                        seen["block_state"].add(b.get("state_id"))
    seen["block"].discard("")
    seen["item"].discard("")
    if not seen["entity_type"] or not seen["block"]:
        die("recordings produced no entity types or no blocks — the field names have moved")
    return seen, unknown, scen, tally


def block_schema_reach(bj, seen_state_ids):
    """The block-state denominator, decomposed into units that are work queues.

    `block_state` is 32,366 and the corpus reaches 87 of them — 0.27%, the
    lowest number this census prints and, read alone, its most alarming. It is
    an artefact of how block states are counted, and the proof is arithmetic
    rather than an opinion:

    **for every one of the 1,196 blocks, `len(states)` is exactly the product
    of its property domain sizes, with zero exceptions.** Nothing is pruned;
    no state is unreachable by construction; there is no information in
    32,366 that is not already in the list of distinct property-set schemas
    and how many blocks share each. A wall is 3*3*3*3*2*2 = 324 states, the
    32 wall blocks are 10,368 of the denominator — 32.0% of it — and all
    10,368 come out of one `WallBlock.makeWallState` and one
    `WallBlock.shouldRaisePost`. Enumerating them is enumerating a Cartesian
    product, and a corpus that placed every wall in every state would have
    written 10,368 checks for two branches.

    That identity is asserted here rather than described, because it is the
    whole argument and a future registry could break it. If Mojang ever
    prunes a state table this refuses instead of quietly reporting a unit
    that has stopped meaning what it says.

    Three replacement units, all from the same report:

      block_schema     distinct property-set signatures      (116)
      block_property   distinct property NAMES               (93)
      block_prop_value distinct (property, value) pairs      (435)

    `block_schema` is the one to quote. A schema is one placement path and one
    `updateShape`; two blocks with the same schema are the same work twice.
    On the same six recordings that score 87/32,366 = 0.27% it scores
    31/116 = 26.72%, and the gap between those two readings is a factor of
    99. Neither is wrong — they are different questions — but only one of them
    is a list of things somebody can go and build.

    The numerator is measured, not asserted: a schema counts as reached when
    some recording carried a state id belonging to a block with that schema.
    """
    schema_of, prop_dom = {}, defaultdict(set)
    sid_block, unpruned = {}, True
    for name, v in bj.items():
        props = v.get("properties", {})
        prod = 1
        for p, vals in props.items():
            prod *= len(vals)
            prop_dom[p].update(vals)
        if len(v["states"]) != prod:
            unpruned = False
        schema_of[name] = tuple(sorted((p, tuple(sorted(w))) for p, w in props.items()))
        for st in v["states"]:
            sid_block[st["id"]] = name
    if not unpruned:
        die("blocks.json no longer satisfies states == product(domains) — the "
            "argument in docs/PARITY-ROADMAP.md §41 rests on that identity and "
            "this unit must be re-derived before it is quoted")

    hit_schema, hit_prop, hit_pv = set(), set(), set()
    for s in seen_state_ids:
        name = sid_block.get(s)
        if name is None:
            continue
        hit_schema.add(schema_of[name])
        for p, vals in bj[name].get("properties", {}).items():
            hit_prop.add(p)
        for p, val in next(
            (st.get("properties", {}) for st in bj[name]["states"] if st["id"] == s), {}
        ).items():
            hit_pv.add((p, val))
    return {
        "block_schema": (len(hit_schema), len(set(schema_of.values()))),
        "block_property": (len(hit_prop), len(prop_dom)),
        "block_prop_value": (len(hit_pv), sum(len(v) for v in prop_dom.values())),
    }


def harness_world(path=None):
    """The corpus's declared world configuration, parsed from `ci/t2-gameplay.sh`.

    Parsed rather than copied. A table of "the gamerules we set" maintained
    beside the script that sets them is a caveat list, and this project's
    caveat lists lag their code — `worldgen-dump`'s did by a whole commit.
    """
    src = open(path or T2).read()
    rules = dict(re.findall(r"^gamerule_or_die\s+\"\$VAN_DIR\"\s+(\S+)\s+(\S+)\s*$", src, re.M))
    props = dict(re.findall(r"(?m)^([a-z-]+)=([A-Za-z0-9_.-]+)\\?n?", ""))
    m = re.search(r"VANILLA_EXTRA_PROPS=\$'([^']*)'", src)
    if m:
        for kv in m.group(1).split("\\n"):
            if "=" in kv:
                k, v = kv.split("=", 1)
                props[k.strip()] = v.strip()
    if len(rules) < 3:
        die(f"parsed only {len(rules)} gamerules out of {os.path.relpath(T2, ROOT)}")
    return rules, props


def scenario_gamerules(checks=None, global_rules=None):
    """Game rules the corpus's own scenarios turn on, per `[[gamerule]]`.

    A section is "provably off" only while **nothing in the corpus turns its
    rule back on**. `ci/t2-gameplay.sh`'s four settings are the substrate, and
    since 2026-08-12 a scenario may declare a rule it needs for the length of
    its own measured window (`scenario::GameRule`) — so the harness's global
    value is no longer the whole of the answer, and a census that read only
    the script would keep reporting a section dark that the corpus now runs.

    Returns `{rule: [(scenario file, value), ...]}` for every declared value
    that is not itself an off value: a scenario setting a rule to `false` or
    `0` has not lit anything up.

    **Read from the check files rather than from a list beside them**, for the
    reason `harness_world` is parsed rather than copied.

    It also refuses the one mistake that would cost the whole corpus: a
    scenario whose `restore` is not what the harness set puts every later
    check on a substrate nobody declared. That is a static property of the
    files and there is no reason to wait for a run to find it.
    """
    import tomllib

    out = defaultdict(list)
    paths = sorted(glob.glob(os.path.join(checks or CHECKS, "*.toml")))
    if not paths:
        die(f"no scenarios at {os.path.relpath(checks or CHECKS, ROOT)}")
    for p in paths:
        with open(p, "rb") as f:
            try:
                doc = tomllib.load(f)
            except Exception as e:  # noqa: BLE001 - reported, not swallowed
                die(f"{os.path.relpath(p, ROOT)}: {e}")
        for g in doc.get("gamerule", ()):
            rule, value, restore = g.get("rule"), str(g.get("value")), str(g.get("restore"))
            if global_rules is not None and rule in global_rules:
                if restore != global_rules[rule]:
                    die(
                        f"{os.path.relpath(p, ROOT)}: [[gamerule]] {rule} restores to "
                        f"{restore!r} but ci/t2-gameplay.sh sets it to "
                        f"{global_rules[rule]!r} — every scenario after this one would "
                        f"run on a substrate nobody declared"
                    )
            if value not in ("false", "0"):
                out[rule].append((os.path.basename(p), value))
    return out


# --------------------------------------------------------------------------
# the weak instrument, kept and labelled
# --------------------------------------------------------------------------


def packet_witness(graph, sec_calls, emitted_classes, cb_classes):
    """Which clientbound packets each section's code can construct. Collapses.

    Reported only under `--weak`. See the module docstring: the resolved graph
    is one strongly-connected component and this separates almost nothing.
    """
    out = {}
    for s, targets in sec_calls.items():
        roots = set()
        for t in targets:
            roots |= graph.resolve(t)
        w = set()
        for m in graph.reach(roots):
            w |= graph.news.get(m, set()) & cb_classes
        out[s] = w
    dark = sorted(s for s, w in out.items() if w and not (w & emitted_classes))
    silent = sorted(s for s, w in out.items() if not w)
    return out, dark, silent


# --------------------------------------------------------------------------


def load_surface():
    """`ci/corpus-surface.py` as a module — it owns the protocol unit.

    Imported rather than re-implemented so that the two scripts cannot drift on
    what "emitted" means. That has already gone wrong once on this project in
    the other direction, where two copies of a floor disagreed.
    """
    p = os.path.join(ROOT, "ci", "corpus-surface.py")
    spec = importlib.util.spec_from_file_location("corpus_surface", p)
    mod = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(mod)
    except Exception as e:  # noqa: BLE001
        die(f"could not load ci/corpus-surface.py: {e}")
    return mod


EXPLAIN = """\
UNIT, PROVENANCE AND EXCLUSIONS
===============================

WHAT IS BEING COUNTED
  Four denominators, none of them ours:

  protocol   the 141 clientbound play packet types `GameProtocols` registers
             in the pinned server-26.2.jar, in registration order.
  tick       the named profiler sections Mojang compiled into the code
             reachable from `MinecraftServer.tickServer` — vanilla's own
             decomposition of what one server tick does.
  registry   `generated/reports/registries.json` from the same jar: entity
             types, blocks, block states, block entity types, menus, sound
             events.
  behaviour  the jar-derived block behaviour tables, scored by
             `ci/corpus-surface.py` (not re-scored here).

WHERE THE NUMERATORS COME FROM
  Recordings of VANILLA, not of us: `oracle-work/gameplay-selfcheck/r*.json`,
  written by `tools/oracle-runner` while the corpus ran against the pinned jar.
  Every numerator is a count of things vanilla was observed to do or contain.
  Nothing is read out of `crates/`, and nothing here boots a server.

  The one numerator that is *not* a recording is the `checks` column of the
  protocol unit, which counts our own `ci/checks/gameplay/*.toml`. It is
  labelled as such wherever it appears.

EXCLUSIONS, AND WHAT EACH COSTS THE CLAIM
  * Eleven clientbound packet types are made ambient by `trace::FAMILIES`
    before any decoder sees them. Vanilla certainly sends most of them every
    run. They are counted as never-emitted, which understates `emitted` by up
    to eleven and is the conservative direction for a coverage claim.
    `--housekeeping-emitted` prints the other bound. It was twelve until
    2026-08-12: `LEVEL_CHUNK_WITH_LIGHT` left the list so that the
    block-entity unit could have a channel the other 29 of its 49 types are
    actually on. The count is scraped from `trace.rs` by
    `ci/corpus-surface.py`, so this sentence is the only place it can go
    stale — the numbers below cannot.
  * Items ARE censused since 2026-08-12, and were not before. `ItemStack` used
    to sit inside the digested tail of every packet that carried one; the arms
    in `trace::read_item_stack` decode it, so the 1,537-entry item registry
    now has a numerator that is a measurement. What is still excluded is
    narrower and is printed: a stack whose component patch adds a component
    with no codec here stops the enclosing packet, so any later stack in that
    same packet goes unread, and an `ItemStack` carried inside entity metadata
    (`set_entity_data`) has no decoder at all. Recordings taken before that
    date carry no item fields and this script refuses them rather than
    reporting their zero.
  * Serverbound packets are counted by `ci/corpus-surface.py` (21 of 69 can be
    sent at all) and not here.
  * The `tick` unit's `unclassified` tier means UNKNOWN. It is not coverage.
  * Reflection and service loading are invisible to `ci/jarcallgraph.py`. No
    conclusion below rests on a call-graph negative; the two sharp rules are a
    game-rule gate and an absent mob.

THE CEILING RULE, PER UNIT — ADDED 2026-08-12
  Every row prints `observed / ceiling / registry`. The ceiling is what the
  corpus could EVER observe; the registry size is only ever an upper bound on
  it, and on two rows it is more than twice the real one. Each rule is stated
  here because each rule is different, and a single word like "reachable"
  covering five of them would be five claims wearing one name.

  Ceilings do not add up and are never summed. Each carries an OWNER:

    jar      vanilla's own construction. Not ours, not spendable, not a backlog.
    ours     this harness or this server. Work, and usually cheaper than a
             scenario — the block-entity row's 29 blocked types are one decoder
             arm, where a scenario buys one type.
    ?        UNKNOWN. The registry size is an upper bound and nothing better is
             known. Deliberately not filled in with a guess.

  clientbound packets   RULE: is the packet class constructed anywhere in the
      pinned jar, by `new` or by a `::new` reference reaching a codec? A type an
      unmodified server never builds cannot be sent by one. THE RULE FIRES ON
      NOTHING — all 139 types whose class GameProtocols resolves are built
      somewhere — so vanilla sets no ceiling here below 141, and the whole
      reduction to 129 is `trace::FAMILIES` making 12 ambient before any
      decoder sees them. OURS.

  tick sections         RULE: none is needed. This unit has no liveness
      numerator at all — nothing here boots a server — and its two measurements
      are negatives. What the ceiling question answers is who owns them, and
      every provably-dead section is dead because of a game rule or a spawn
      setting in `ci/t2-gameplay.sh`. Zero are dead by vanilla's construction.
      OURS, all of it.

  block entity types    RULE, PER CHANNEL, and the owner is the whole point.
      `block_entity_data`: does the registered type's class override
      `BlockEntity.getUpdatePacket()`? The base is `aconst_null; areturn` and
      `ChunkHolder.broadcastBlockEntity` returns on null, so 20 of 49 can ever
      be its subject. The chunk payload: NO FILTER — `ClientboundLevelChunkPacketData`
      carries a `BlockEntityInfo` for every block entity in the chunk, so all
      49. Vanilla's ceiling on this unit is therefore 49, not 20. The census
      reaches 20 only because `trace::FAMILIES` makes ambient
      LEVEL_CHUNK_WITH_LIGHT and nothing decodes it, which is a recorder
      decision: **OURS**, and one decoder arm buys all 29. This row was
      published as JAR until 2026-08-16; §29.7 is the correction and
      `ceiling_premise_housekeeping` is the guard that stops the premise
      rotting again. §25, §29.7.

  menu types            RULE: is the menu's registered factory called from any
      class but `MenuType` itself? Four — generic_9x1, _9x2, _9x4, _9x5 — are
      named by exactly one class in the whole jar, the registration site. The
      control is in the same scan: `ChestMenu.threeRows` has five callers. A
      fifth, `merchant`, has an opener that only an entity implements, and this
      harness boots spawn_mobs=false / spawn-monsters=false / peaceful. So 21
      is vanilla's ceiling and 20 is this harness's. JAR + OURS. §28.

  sound events          CITED, not derived, and the only such number here.
      256 of 1,968 are reachable, from `crates/rustmc-server/src/sounds.rs` and
      `world_send::place_sound`, the only emitters: a scenario listening for
      anything else reports `absent` and adjudicates nothing. That makes it a
      ceiling on OUR emitter rather than on vanilla, so it is the one figure in
      this file that goes stale when our own source changes — re-derive it when
      that file does. VANILLA'S OWN SOUND CEILING IS UNKNOWN. §27.1.

  entity types          UNKNOWN, and stated rather than invented. With a
      creative hotbar, blocks and items, most of the 158 could in principle be
      made to appear, and enumerating the ones that could not would be
      invention. What is readable off Mojang's item registry is a FLOOR on the
      ceiling: the entity types with a spawn egg, which a scenario reaches with
      one hotbar slot and no new verb. The binding constraint on the observed 4
      is spawn_mobs=false / spawn-monsters=false / peaceful, which is OURS.

  blocks, block states  UNKNOWN. No jar rule was found and two were tried and
      rejected: "has a BlockItem" is a floor and not a ceiling (fire, farmland,
      dirt_path and water all arrive without one), and `net/minecraft/
      references/BlockItemIds` is a datafixer list of 787 legacy names and not
      an authority on 26.2. The reducers that can be named are all ours: a
      superflat overworld, generate-structures=false, and no mobs.

  A RULE THAT WAS TRIED AND REFUSED, recorded so nobody rebuilds it: for sound
  events, "no class but `SoundEvents` reads the field" excludes 131 of 1,968 and
  is NOT SOUND. Sound ids also reach `playSound` by name from data — jukebox
  songs and `sound_types.json` — so an unreferenced field is not a proof of
  unreachability, and 94 registry entries have no `SoundEvents` field at all.

FLOOR
  This measurement has no floor in the corpus-floor sense and cannot have one:
  it is a census, not a comparison, so there is no pair of vanilla runs whose
  disagreement would bound it. What it has instead is a REPRODUCIBILITY check —
  the same six recordings are censused independently and the per-run reach is
  printed, so a numerator that moves between runs of the same vanilla is
  visible. Quote the per-run spread beside any figure from this script; a
  census with no spread stated is a census asserting a stability it did not
  measure.
"""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--jar", default=JAR)
    ap.add_argument(
        "--recordings",
        default="~/rustmc-cov2/oracle-work/gameplay-selfcheck/r[0-9]*.json",
        help="glob of vanilla recorder outputs to census",
    )
    ap.add_argument("--explain", action="store_true", help="unit, provenance, exclusions; then exit")
    ap.add_argument("--weak", action="store_true", help="also print the collapsed packet-witness instrument")
    ap.add_argument("--housekeeping-emitted", action="store_true",
                    help="count the housekeeping types as emitted (a reading, not a measurement)")
    ap.add_argument("--json", help="write the census as JSON to this path")
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()

    if args.explain:
        print(EXPLAIN)
        return 0
    if args.selftest:
        return selftest()

    sys.path.insert(0, os.path.join(ROOT, "ci"))
    from jarcallgraph import JarGraph  # noqa: PLC0415

    cs = load_surface()
    regs = registries()
    files = recordings(args.recordings)

    # ---- unit: protocol -------------------------------------------------
    jar_cb, jar_sb = cs.jar_protocol(args.jar)
    housekeeping, decoded = cs.trace_sets()
    # Before any ceiling is computed, not after: a ceiling rule that spends a
    # packet drop is only sound while the drop is there, and this is the one
    # place the census reads the list it has been asserting about.
    ceiling_premise_housekeeping(housekeeping)
    checks = cs.load_checks()
    used = defaultdict(list)
    for fn, c in checks.items():
        for k in c.get("observe", []):
            used[k].append(fn)
    _f, emitted, _w, _p = cs.recordings_census(args.recordings)
    ours = {v: k for k, v in cs.ALIASES.items()}
    rows = []
    for i, name in enumerate(jar_cb):
        mine = ours.get(name, name)
        if mine in housekeeping:
            tier, kind = "housekeeping", None
        elif mine in decoded:
            tier, kind = "legible", decoded[mine]
        else:
            tier, kind = "opaque", f"packet:0x{i:02X}"
        n_emit = emitted.get(kind, 0) if kind else 0
        if args.housekeeping_emitted and tier == "housekeeping":
            n_emit = 1
        rows.append({"id": i, "name": name, "tier": tier, "kind": kind,
                     "emitted": n_emit, "checks": len(used.get(kind, [])) if kind else 0})
    n_emit = sum(1 for r in rows if r["emitted"])
    n_obs = sum(1 for r in rows if r["checks"])
    n_both = sum(1 for r in rows if r["emitted"] and r["checks"])

    # ---- unit: tick -----------------------------------------------------
    graph = JarGraph(args.jar)
    sections, pushers, sec_calls, reach, nameless = tick_sections(graph)
    rule_ids, _ = reg_names(regs, "minecraft:game_rule")
    rules_set, props = harness_world()

    callers = defaultdict(set)
    for m in reach:
        for t in graph.calls.get(m, ()):
            if (t[1], t[2]) in graph.declares.get(t[0], ()):
                callers[(t[0], t[1], t[2])].add(m)

    # NEGATIVE CONTROL. A verifier that passes everything is not a verifier, and
    # this one walks a neighbourhood rather than a data-flow, so it has to be
    # shown refusing a pair that is wrong. `randomTick` is gated by
    # `RANDOM_TICK_SPEED` and by nothing about the weather.
    if rule_is_gated(graph, pushers, "randomTick", "advance_weather", callers):
        die("negative control failed: the gate verifier accepts advance_weather -> randomTick")

    # What the corpus's own scenarios turn back on. A rule the harness sets to
    # an off value and a scenario sets to an on value is **not** a rule that
    # proves anything dark: vanilla's section runs, inside that scenario's
    # measured window, and the section moves from PROVABLY OFF to unclassified
    # — which is *unknown*, not covered. See §21.4: the positives here are
    # presence and this is not even that. What it stops being is a negative.
    reenabled = scenario_gamerules(global_rules=rules_set)

    gated, unverified = {}, []
    for rule, secs in RULE_SECTIONS.items():
        if f"minecraft:{rule}" not in rule_ids:
            die(f"game rule '{rule}' is not in the pinned jar's registry — 26.2 renamed it")
        if rule not in rules_set:
            continue
        off = rules_set[rule] in ("false", "0") and rule not in reenabled
        for s in secs:
            if s not in sections:
                unverified.append((s, rule, "no such section under the tick"))
            elif not rule_is_gated(graph, pushers, s, rule, callers):
                unverified.append((s, rule, f"section does not reference GameRules.{rule.upper()}"))
            elif off:
                gated[s] = rule

    ent_names, _ = reg_names(regs, "minecraft:entity_type")
    seen, unknown, scen, tally = registry_reach(files, regs)
    no_pop, mob_unresolved = {}, []
    for s in sections:
        m = MOB_SUFFIX.match(s)
        if not m or not m.group(1):
            continue
        eid = "minecraft:" + camel_to_snake(m.group(1))
        if eid not in ent_names:
            mob_unresolved.append((s, eid))
            continue
        if eid not in seen["entity_type"]:
            no_pop[s] = eid
    if len(no_pop) < 15:
        die(f"only {len(no_pop)} per-mob sections resolved — the naming convention moved")

    unclassified = sorted(set(sections) - set(gated) - set(no_pop))

    # ---- output ---------------------------------------------------------
    print(f"pinned jar   {os.path.relpath(args.jar, ROOT)}")
    print(f"recordings   {len(files)} vanilla runs, {len(scen)} scenarios, from {args.recordings}")
    print(f"corpus       {len(checks)} checks under ci/checks/gameplay")
    print(f"STAMP        {stamp(args.jar, files, len(checks))}")
    print(f"world        {', '.join(f'{k}={v}' for k, v in sorted(rules_set.items()))}")
    print(f"             {', '.join(f'{k}={v}' for k, v in sorted(props.items()))}")
    print()

    # ---- ceiling: protocol ----------------------------------------------
    cls_of = packet_classes(args.jar, jar_cb)
    unbuildable = ceiling_packets(graph, cls_of)
    pkt_ceiling = len(jar_cb) - len(housekeeping) - len(unbuildable)

    print("== UNIT 1 — clientbound play packet types (denominator: GameProtocols) ==")
    print(f"  {len(jar_cb):5d}  registered in the protocol")
    print(f"  {len(jar_cb) - len(unbuildable):5d}  CEILING, vanilla's — types an unmodified server can construct"
          f"  ({len(unbuildable)} excluded)")
    print(f"  {pkt_ceiling:5d}  CEILING, OURS — minus the {len(housekeeping)} `trace::FAMILIES` makes ambient"
          f" before any decoder")
    print(f"  {n_emit:5d}  vanilla EMITTED during the corpus"
          + ("  (incl. housekeeping, asserted)" if args.housekeeping_emitted else ""))
    print(f"  {n_obs:5d}  named by >= 1 check")
    print(f"  {n_both:5d}  emitted AND named")
    print(f"  -> {n_obs}/{pkt_ceiling}/{len(jar_cb)} = {100*n_obs/pkt_ceiling:.1f}% of the reachable ceiling,"
          f" {100*n_obs/len(jar_cb):.1f}% of the protocol")
    print(f"  -> {n_both}/{n_emit} = {100*n_both/n_emit:.1f}% of what the corpus provokes")
    print("  -> the vanilla rule (never constructed in the jar) excludes"
          f" {len(unbuildable)}: every reduction on this unit is ours")
    print()

    print("== UNIT 2 — vanilla's own per-tick sections (denominator: the jar's profiler) ==")
    print(f"  {len(sections):5d}  named sections reachable from MinecraftServer.tickServer")
    print(f"  {nameless:5d}  push sites with a non-constant name (excluded, not counted above)")
    print(f"  {len(gated):5d}  PROVABLY OFF — vanilla's own game-rule gate, shut by this harness")
    print(f"  {len(no_pop):5d}  PROVABLY UNPOPULATED — per-mob, and no recording ever held that mob")
    print(f"  {len(unclassified):5d}  unclassified — UNKNOWN, not covered")
    ev = len(gated) + len(no_pop)
    print(f"  -> {ev}/{len(sections)} = {100*ev/len(sections):.1f}% of vanilla's tick is provably never run")
    # The three-number reading for this unit is not observed/ceiling/registry,
    # because there is no liveness numerator here at all — nothing boots a
    # server, and the only measurements are two NEGATIVES. What the ceiling
    # question does answer is who owns them, and the answer has no ambiguity in
    # it: every section this census can prove dead is dead because of a setting
    # in `ci/t2-gameplay.sh`. Not one of them is dead by vanilla's own
    # construction, so the whole of this unit's shortfall is ours to move.
    print(f"  -> observed: not measured (no server is booted). Of the {ev} provably dead,"
          f" {ev} are OURS ({len(gated)} game-rule, {len(no_pop)} spawn configuration)"
          " and 0 are vanilla's.")
    print(f"  -> so the harness ceiling on live sections is {len(sections) - ev}/{len(sections)},"
          " and vanilla's ceiling is the whole 120.")
    print()
    for s, rule in sorted(gated.items()):
        print(f"     off   {s:28s} gamerule {rule}={rules_set[rule]}")
    # Printed even though it is not counted anywhere, because "this section
    # stopped being provably dead" is otherwise invisible: it leaves the
    # PROVABLY OFF line and arrives in `unclassified`, which is the same place
    # 79 other sections nobody has said anything about already sit.
    for rule, who in sorted(reenabled.items()):
        for s in RULE_SECTIONS.get(rule, ()):
            if s in sections and s not in gated:
                for f, v in who:
                    print(f"     LIT   {s:28s} gamerule {rule}={v} for {f}")
    if unverified:
        print("     -- asserted gate pairs that did NOT verify against the jar, and are not counted:")
        for s, rule, why in unverified:
            print(f"        {s:28s} {rule:20s} {why}")
    if mob_unresolved:
        print(f"     -- {len(mob_unresolved)} mob-shaped section names did not resolve to an entity type"
              f" (left unclassified): {', '.join(s for s, _ in mob_unresolved[:6])}")
    print()

    # ---- ceilings: the registry rows ------------------------------------
    be_names_all, _ = reg_names(regs, "minecraft:block_entity_type")
    be_chan = ceiling_block_entities(graph, args.jar, set(be_names_all))

    be_reach, be_all, be_owner, be_live_chan, be_dark = live_ceiling(be_chan, housekeeping, decoded)
    be_blocked = be_all - be_reach

    menus = ceiling_menus(graph)
    menu_no_opener = sorted(m for m, (v, _c) in menus.items() if v == "no-opener")
    menu_entity_only = sorted(m for m, (v, _c) in menus.items() if v == "entity-only")
    eggs = spawn_egg_types(regs)
    ent_floor = len(eggs | seen["entity_type"])

    # `ceil` is the number the row can never exceed; `owner` says whose it is;
    # `rule` is the one line that has to survive a version bump. `None` is
    # UNKNOWN and prints as `?` — never as the registry size, which would make
    # an unbounded row look bounded.
    ceilings = {
        "sound_event": (CITED_SOUND_CEILING["value"], "ours",
                        "cited " + CITED_SOUND_CEILING["cite"] + "; vanilla's own is UNKNOWN"),
        "block": (None, "unknown", "no jar rule found — see --explain for the two that were tried"),
        "entity_type": (None, "unknown",
                        f"no jar rule caps it; >= {ent_floor} are one hotbar slot away (spawn eggs)"),
        "block_entity_type": (
            len(be_reach), be_owner,
            "per channel: " + ", ".join(
                f"{n}({p}{'' if n in be_live_chan else ' DROPPED'})={len(i)}"
                for n, (p, i) in sorted(be_chan.items())
            ) + f"; live union {len(be_reach)}, vanilla's {len(be_all)}"
            + (f"; the {len(be_blocked)} gap is OURS — " +
               ", ".join(p for _n, p in be_dark) + " is not decoded" if be_owner == "ours" else "")
        ),
        "menu": (len(menus) - len(menu_no_opener) - len(menu_entity_only), "jar+ours",
                 f"vanilla's is {len(menus) - len(menu_no_opener)} ({len(menu_no_opener)} have no opener"
                 f" anywhere in the jar); this harness's is"
                 f" {len(menus) - len(menu_no_opener) - len(menu_entity_only)},"
                 f" {len(menu_entity_only)} more needing a mob (ours)"),
        "block_state": (None, "unknown", "no jar rule found; inherits the block row's"),
    }

    print("== UNIT 3 — vanilla's registries (denominator: reports/registries.json) ==")
    reg_rows = [
        ("sound_event", "minecraft:sound_event"),
        ("block", "minecraft:block"),
        ("entity_type", "minecraft:entity_type"),
        ("block_entity_type", "minecraft:block_entity_type"),
        ("menu", "minecraft:menu"),
    ]
    print(f"  {'registry':20s} {'observed':>8s} {'ceiling':>8s} {'registry':>8s} "
          f"{'of ceil':>8s} {'of reg':>7s}  owner")
    reg_out = {}

    def reg_line(label, t, d):
        reg_out[label] = (t, d)
        c, owner, _rule = ceilings.get(label, (None, "unknown", ""))
        cs = f"{c:8d}" if c is not None else f"{'?':>8s}"
        pc = f"{100*t/c:7.2f}%" if c else f"{'—':>8s}"
        print(f"  {label:20s} {t:8d} {cs} {d:8d} {pc} {100*t/d:6.2f}%  {owner}")

    for label, key in reg_rows:
        names, _ = reg_names(regs, key)
        reg_line(label, len(seen[label]), len(names))
    blocks_json = os.path.join(REPORTS, "blocks.json")
    if os.path.exists(blocks_json):
        bj = json.load(open(blocks_json))
        nstates = sum(len(v["states"]) for v in bj.values())
        t = len(seen["block_state"])
        reg_out["block_state"] = (t, nstates)
        print(f"  {'block_state':20s} {t:8d} {nstates:8d} {100*t/nstates:7.2f}%   {nstates-t}")
        for label, (t2, d2) in block_schema_reach(bj, seen["block_state"]).items():
            reg_out[label] = (t2, d2)
            print(f"  {label:20s} {t2:8d} {d2:8d} {100*t2/d2:7.2f}%   {d2-t2}")
        print("  ^^ block_state is a CARTESIAN PRODUCT and is not a work queue —"
              " docs/PARITY-ROADMAP.md §41")
    if unknown:
        print(f"  (ids with no registry entry, per bucket: {dict(unknown)})")
    # THE ITEM ROW, AND THE ONE THING IT MUST NOT DO.
    #
    # It printed `— 1537 — not legible` until 2026-08-12, because every
    # `ItemStack` was inside `rest`. It is legible now, and the risk moved
    # rather than disappearing: a recording taken *before* the decoder has no
    # item fields, and censusing one would print `0 / 1537  0.00%`, which is a
    # reading of the recorder wearing the clothes of a measurement. So the
    # numerator is only printed when the recordings prove they could carry
    # one — `item_bearing` counts observations of the five kinds whose arms
    # decode a stack — and otherwise this refuses, in the same shape as every
    # other extraction in this script.
    item_names, _ = reg_names(regs, "minecraft:item")
    d = len(item_names)
    if tally["item_bearing"] and not tally["item_stacks"]:
        print(f"  {'item':20s} {'—':>8s} {d:8d} {'—':>8s}   "
              f"{tally['item_bearing']} stack-bearing observations carried no decoded stack:"
              f" these recordings pre-date the ItemStack decoder")
    elif not tally["item_bearing"]:
        print(f"  {'item':20s} {'—':>8s} {d:8d} {'—':>8s}   "
              f"the corpus produced no stack-bearing packet at all")
    else:
        t = len(seen["item"])
        reg_out["item"] = (t, d)
        print(f"  {'item':20s} {t:8d} {d:8d} {100 * t / d:7.2f}%   {d - t}")
        print(f"  (item stacks decoded: {tally['item_stacks']} over"
              f" {tally['item_bearing']} stack-bearing observations;"
              f" {tally['item_opaque']} stopped at a data component with no codec)")
        # THE CEILING, PRINTED WHERE THE RATIO IS, BECAUSE THAT IS THE ONLY
        # PLACE IT STOPS THE RATIO BEING MISREAD. See `scenario_vocabulary`.
        vocab = scenario_vocabulary(regs)
        v = len(vocab)
        reached = len(seen["item"] & vocab)
        print(f"  (the corpus's item VOCABULARY is {v} of {d} — its `hold` steps name"
              f" {v} distinct items and no")
        print(f"   recording can reach one they do not name, so {v} is this corpus's"
              f" ceiling and the")
        print(f"   recordings add {reached} of it. {100 * t / d:.2f}% is a vocabulary"
              f" measured against a registry,")
        print( "   NOT a queue being worked through. Counted from"
               " ci/checks/gameplay/*.toml, not from a recording.)")
        if seen["item"] - vocab:
            print(f"   -- {len(seen['item'] - vocab)} observed item(s) are outside the"
                  f" `hold` vocabulary — vanilla produced them")
            print(f"      itself rather than being handed them:"
                  f" {', '.join(sorted(seen['item'] - vocab))}")
    print()

    print("== CEILINGS — the rule for each row, and whose ceiling it is ==")
    print("  A `jar` ceiling is a fact about vanilla and is nobody's to spend money on.")
    print("  An `ours` ceiling is work, and usually cheaper work than a scenario.")
    print("  A `?` is UNKNOWN: the registry size is an upper bound and nothing better is known.")
    for label, _key in reg_rows + [("block_state", None)]:
        if label not in ceilings:
            continue
        c, owner, rule = ceilings[label]
        print(f"  {label:20s} {('%d' % c) if c is not None else '?':>6s}  {owner:9s} {rule}")
    print(f"  {'clientbound packet':20s} {pkt_ceiling:6d}  {'ours':9s} "
          f"{len(unbuildable)} unbuildable in the jar (vanilla's),"
          f" {len(housekeeping)} made ambient by trace::FAMILIES (ours)")
    print(f"  {'tick section':20s} {len(sections)-ev:6d}  {'ours':9s} "
          f"all {ev} provably-dead sections are shut by ci/t2-gameplay.sh, none by vanilla")
    print()
    be_null = be_all - be_chan["block_entity_data"][1]
    print(f"     block entity — reachable on a LIVE channel ({len(be_reach)}): "
          f"{', '.join(sorted(x.split(':')[1] for x in be_reach))}")
    print(f"     block entity — vanilla's own ceiling is {len(be_all)}, NOT {len(be_reach)}: the "
          f"{len(be_null)} that inherit a null")
    print("                    getUpdatePacket are still carried in the chunk payload, as "
          "BlockEntityInfo, unfiltered.")
    for name, pkt in be_dark:
        print(f"                    channel `{name}` ({pkt}) is not decoded by trace.rs, so it is dark"
              f" — {'ambient by declaration' if pkt in housekeeping else 'kept but undecoded'}.")
    print(f"                    The {len(be_blocked)}-type shortfall is therefore OURS and one decoder"
          " arm buys all of it (§25.2, §29.4).")
    print(f"     menu — no opener anywhere in the jar ({len(menu_no_opener)}): "
          f"{', '.join(x.split(':')[1] for x in menu_no_opener)}")
    print(f"     menu — an opener, but only an entity's ({len(menu_entity_only)}): "
          f"{', '.join(x.split(':')[1] for x in menu_entity_only)}"
          "  — spawn_mobs=false, difficulty=peaceful")
    print()

    # per-run reach — the reproducibility statement the census needs
    print("== reproducibility — the same census, one recording at a time ==")
    spread = defaultdict(list)
    # `item` joins the loop only when the pooled row was printable, so a
    # pre-decoder ensemble cannot produce a per-run spread of `0..0` that
    # reads like a stable measurement of nothing.
    repro_rows = list(reg_rows) + ([("item", "minecraft:item")] if "item" in reg_out else [])
    for p in files:
        s1, _u, _sc, _t = registry_reach([p], regs)
        for label, _k in repro_rows:
            spread[label].append(len(s1[label]))
        # **The block rows are here so that a one-recording census can be
        # compared with a six-recording one.** A branch that adds scenarios has
        # exactly one fresh `oracle-work/gameplay/vanilla.json` and the
        # published before-figures are pooled over an ensemble, so the
        # comparison is only legitimate if a single run already saturates these
        # units. Measured 2026-08-12 over `coverage92-selfcheck-n6-20260811`:
        # every one of the six recordings reaches 87 states and 31 schemas on
        # its own, and the pool adds nothing. That is a property of a corpus
        # that provokes the same block changes every run, not a law — so it is
        # printed rather than assumed, and a spread appearing here is the
        # signal that a before/after across different N has stopped being fair.
        if "block_state" in reg_out:
            spread["block_state"].append(len(s1["block_state"]))
            for lab, (t2, _d2) in block_schema_reach(bj, s1["block_state"]).items():
                spread[lab].append(t2)
    extra = [l for l in ("block_state", "block_schema", "block_property",
                         "block_prop_value") if l in spread]
    for label in [l for l, _k in reg_rows] + extra:
        v = spread[label]
        print(f"  {label:20s} per-run {min(v)}..{max(v)}   pooled {reg_out[label][0]}")
    print()

    # The count is not in this heading on purpose. It said "five units" while
    # printing six, which is the same shape of stale figure this file's own
    # UNIT 3 note warns about.
    print("== SENSITIVITY — the same corpus, every unit above ==")
    sens = [
        ("clientbound types the corpus provokes, named by a check", n_both, n_emit),
        ("clientbound types in the protocol, named by a check", n_obs, len(jar_cb)),
        ("vanilla tick sections not provably dead", len(sections) - ev, len(sections)),
        ("entity types ever present", *reg_out["entity_type"]),
        ("blocks ever seen change", *reg_out["block"]),
        ("sound events ever heard", *reg_out["sound_event"]),
    ]
    # The two readings of one registry, kept adjacent on purpose. They differ
    # by ~99x on identical recordings and neither is wrong; `block_state`
    # counts a Cartesian product and `block_schema` counts the distinct code
    # paths that product is generated by. A reader who sees only the first
    # concludes this corpus has touched nothing.
    for lab, key in (("block STATES ever seen", "block_state"),
                     ("block state SCHEMAS ever seen", "block_schema")):
        if key in reg_out:
            sens.append((lab, *reg_out[key]))
    for label, a, b in sens:
        art = "  (artefact)" if "STATES" in label else ""
        print(f"  {100*a/b:6.2f}%   {a:6d} / {b:<6d}  {label}{art}")
    # **The spread is taken WITHOUT `block_state`, and saying so is the point.**
    # It is the least flattering row by a wide margin and it would set the
    # bottom of this range on its own — but its denominator is a Cartesian
    # product, so a ratio anchored on it is a fact about multiplication. The
    # spread is meant to say how much the choice of *unit* changes the answer,
    # and an artefact unit inflates it for free.
    real = [(l, a, b) for l, a, b in sens if "STATES" not in l]
    hi = max(100 * a / b for _l, a, b in real)
    lo = min(100 * a / b for _l, a, b in real)
    print(f"  -> the answer moves {hi/lo:.0f}x between the most and least flattering"
          f" unit, excluding block_state")
    if "block_state" in reg_out and "block_schema" in reg_out:
        st = 100 * reg_out["block_state"][0] / reg_out["block_state"][1]
        sc = 100 * reg_out["block_schema"][0] / reg_out["block_schema"][1]
        print(f"  -> and {sc/st:.0f}x between the two readings of ONE registry:"
              f" block_state {st:.2f}% against block_schema {sc:.2f}%")
    # The same spread against ceilings rather than registry sizes. Printed
    # beside it rather than instead of it, because the registry reading is the
    # one every earlier section quoted and a reader has to be able to see the
    # unit change rather than find a different number in the same place.
    sens_c = [
        ("clientbound types in the protocol, named by a check", n_obs, pkt_ceiling),
        ("vanilla tick sections not provably dead", len(sections) - ev, len(sections)),
        ("block entity types ever observed", reg_out["block_entity_type"][0], len(be_reach)),
        ("menu types ever opened", reg_out["menu"][0],
         len(menus) - len(menu_no_opener) - len(menu_entity_only)),
        ("sound events ever heard", reg_out["sound_event"][0], CITED_SOUND_CEILING["value"]),
    ]
    print("  -- and against the CEILING instead of the registry, where one is known:")
    for label, a, b in sens_c:
        print(f"  {100*a/b:6.2f}%   {a:6d} / {b:<6d}  {label}")
    hic = max(100 * a / b for _l, a, b in sens_c)
    loc = min(100 * a / b for _l, a, b in sens_c)
    print(f"  -> {hic/loc:.0f}x on the rows that have a ceiling. The spread survives the correction;"
          " what changes is which rows are catastrophic.")
    print()

    print("== RANKED — the largest systems with zero corpus evidence ==")
    print("  Ranked by the gap to the CEILING, with the gap to the registry beside it.")
    print("  A row whose two numbers differ is a row whose old rank was measuring the registry.")
    print("  A `?` row is PLACED by its registry gap because it has no ceiling; the placement")
    print("  of those rows is not a measurement and they may be anywhere in this list.")
    ranked = rank(sections, gated, no_pop, seen, regs, reg_out, ceilings, len(be_reach),
                  len(menus) - len(menu_no_opener) - len(menu_entity_only))
    for i, (name, size, reg_gap, unit, note) in enumerate(ranked, 1):
        gap = f"{size:6d}" if size is not None else f"{'?':>6s}"
        print(f"  {i:2d}. {name:32s} {gap} of {reg_gap:6d} {unit:16s} {note}")
    print()

    if args.weak:
        cb_classes = {c for c in cls_of.values() if c}
        emitted_classes = {cls_of[r["name"]] for r in rows if r["emitted"] and cls_of.get(r["name"])}
        wit, dark, silent = packet_witness(graph, sec_calls, emitted_classes, cb_classes)
        print("== WEAK — packet-witness over the resolved call graph (NO RESOLVING POWER) ==")
        print(f"  {len(dark):5d}  sections whose every constructible packet was never emitted")
        print(f"  {len(silent):5d}  sections that construct no clientbound packet at all")
        print(f"  {max(len(w) for w in wit.values()):5d}  packet types the *largest* section reaches"
              f" — and {len(wit.get('wardenBrain', ())):d} for `wardenBrain`, which is the collapse")
        print(f"  dark: {', '.join(dark)}")
        print()

    if args.json:
        json.dump(
            {
                "jar": os.path.relpath(args.jar, ROOT),
                "recordings": [os.path.abspath(f) for f in files],
                "scenarios": len(scen),
                "protocol": {"denominator": len(jar_cb), "emitted": n_emit,
                             "observed": n_obs, "both": n_both,
                             "ceiling": pkt_ceiling, "ceiling_owner": "ours",
                             "unbuildable": sorted(unbuildable),
                             "housekeeping": sorted(housekeeping)},
                "tick": {"sections": len(sections), "gated_off": sorted(gated),
                         "unpopulated": sorted(no_pop), "unclassified": unclassified,
                         "ceiling": len(sections) - ev, "ceiling_owner": "ours",
                         "dead_owned_by_vanilla": 0},
                "registry": {
                    k: {"touched": v[0], "denominator": v[1],
                        "ceiling": ceilings.get(k, (None, "unknown", ""))[0],
                        "ceiling_owner": ceilings.get(k, (None, "unknown", ""))[1],
                        "ceiling_rule": ceilings.get(k, (None, "unknown", ""))[2]}
                    for k, v in reg_out.items()
                },
                "ceiling_detail": {
                    "block_entity_reachable": sorted(be_reach),
                    "block_entity_unreachable": sorted(be_blocked),
                    "block_entity_inherits_null": sorted(be_null),
                    "block_entity_vanilla_ceiling": len(be_all),
                    "block_entity_channels": {
                        n: {"packet": p, "types": len(i), "live": n in be_live_chan}
                        for n, (p, i) in sorted(be_chan.items())
                    },
                    "housekeeping_pinned": sorted(HOUSEKEEPING_PINNED),
                    "menu_no_opener": menu_no_opener,
                    "menu_entity_only": menu_entity_only,
                    "entity_type_spawn_egg_floor": ent_floor,
                },
                "per_run": {k: v for k, v in spread.items()},
            },
            open(args.json, "w"),
            indent=1,
            sort_keys=True,
        )
        print(f"wrote {args.json}")
    return 0


def rank(sections, gated, no_pop, seen, regs, reg_out, ceilings, be_ceiling, menu_ceiling):
    """The work order. Ranked by the gap to the CEILING, not to the registry.

    Until 2026-08-12 this ranked by `registry size - observed`, and on two rows
    that was the wrong quantity by more than half: block entities read "48 to
    go" against a real 19, and menus "22" against 17. A work order built on a
    denominator nobody can reach is a work order for scenarios that would come
    back `absent`.

    Size is still measured in the unit each system's surface is *enumerated* in
    by Mojang, and the unit is still printed beside the number precisely
    because the numbers are not comparable across rows. Rows whose ceiling is
    unknown rank on `?` and are placed by their registry gap, with the `?`
    visible so that the placement is not read as a measurement.

    The three tick-section rows are OURS in a way none of the others are: they
    are shut by settings in `ci/t2-gameplay.sh`, so their gap and their ceiling
    are the same number and the whole of it is spendable.
    """
    ent_names, _ = reg_names(regs, "minecraft:entity_type")
    be_names, _ = reg_names(regs, "minecraft:block_entity_type")
    snd_names, _ = reg_names(regs, "minecraft:sound_event")
    blk_names, _ = reg_names(regs, "minecraft:block")
    menu_names, _ = reg_names(regs, "minecraft:menu")
    snd_ceiling = ceilings["sound_event"][0]
    rows = [
        ("mob AI and mob behaviour", None, len(ent_names) - len(seen["entity_type"]), "entity types",
         f"{len(no_pop)} per-mob tick sections provably never ran; ceiling unknown, spawning is OURS"),
        ("sound emission", snd_ceiling - len(seen["sound_event"]),
         len(snd_names) - len(seen["sound_event"]), "sound events",
         "ceiling is OUR emitter (§27.1); vanilla's is unknown"),
        ("block behaviour", None, len(blk_names) - len(seen["block"]), "blocks",
         f"{reg_out['block_state'][1] - reg_out['block_state'][0]} block states unseen; no jar rule caps it"
         if "block_state" in reg_out else "no jar rule caps it"),
        # **The note is derived, not written down.** It read "only `sign` has
        # ever been observed" until 2026-08-12, when eleven more types started
        # being observed and the sentence became a caveat lagging its own
        # measurement — the exact failure this file's own §21.4 warns about.
        #
        # **AND NEITHER IS THE CEILING, AS OF 2026-08-16.** The 29/49 is a fact
        # about the jar — 29 of the 49 types inherit
        # `BlockEntity.getUpdatePacket()`, which is `aconst_null; areturn`
        # (`docs/PARITY-ROADMAP.md` §25) — but it is a fact about the
        # `block_entity_data` CHANNEL and never was one about this row. The
        # chunk payload carries every block entity with no such filter. Branch
        # `trace-chunk-block-entities` reached the same conclusion from the
        # other side and left what caps the row as an open question; §29.7
        # answers it rather than asserting it. `be_ceiling` is the union over
        # the channels `trace.rs` actually keeps AND decodes, folded by
        # `live_ceiling`, so this row reads 20 while the chunk packet is
        # dropped and 49 the day it is decoded, without anyone editing a
        # sentence here.
        ("block entities", be_ceiling - len(seen["block_entity_type"]),
         len(be_names) - len(seen["block_entity_type"]), "block entity types",
         f"{len(be_names) - be_ceiling} unreachable on any channel trace.rs decodes (§25, §29.4)"),
        ("natural + custom spawning", sum(1 for s in gated if "pawn" in s),
         sum(1 for s in gated if "pawn" in s), "tick sections",
         "gamerule spawn_mobs=false — OURS"),
        ("random ticks, growth, fire, ice", sum(1 for s in gated if s == "randomTick"),
         sum(1 for s in gated if s == "randomTick"), "tick sections",
         "gamerule random_tick_speed=0 — OURS"),
        ("weather and thunder", sum(1 for s in gated if s in ("weather", "thunder")),
         sum(1 for s in gated if s in ("weather", "thunder")), "tick sections",
         "gamerule advance_weather=false — OURS"),
        ("container menus", menu_ceiling - len(seen["menu"]),
         len(menu_names) - len(seen["menu"]), "menu types",
         f"{len(menu_names) - menu_ceiling} have no opener a block can reach (§28)"),
    ]
    # FILTERED ON THE CEILING GAP, NOT THE REGISTRY GAP — 2026-08-16. This read
    # `if r[2] > 0` and so kept a row alive on its *registry* gap while ranking
    # it on its ceiling gap, which is the same conflation the whole section
    # corrected: a row whose ceiling gap has gone to zero is finished, and
    # listing it as "the largest system with zero corpus evidence" is a work
    # order for scenarios that would come back `absent`. A `?` row still falls
    # back to the registry gap because it has nothing else, and that is stated
    # in the heading rather than hidden here.
    return sorted(
        (r for r in rows if (r[1] if r[1] is not None else r[2]) > 0),
        key=lambda r: -(r[1] if r[1] is not None else r[2]),
    )


def packet_classes(jar, jar_cb):
    """CLIENTBOUND_X -> the packet class registered for it, from GameProtocols."""
    import subprocess  # noqa: PLC0415
    import tempfile  # noqa: PLC0415
    import zipfile  # noqa: PLC0415

    with tempfile.TemporaryDirectory() as td:
        with zipfile.ZipFile(jar) as z:
            z.extract("net/minecraft/network/protocol/game/GameProtocols.class", td)
        out = subprocess.run(
            ["javap", "-p", "-c", "-constants",
             os.path.join(td, "net/minecraft/network/protocol/game/GameProtocols.class")],
            capture_output=True, text=True, check=False).stdout
    toks = re.findall(
        r"// Field [a-z/]+/\w+PacketTypes\.((?:CLIENTBOUND|SERVERBOUND)_[A-Z_0-9]+)"
        r"|// Field ([a-z/]+)/([A-Za-z0-9$]+)\.STREAM_CODEC", out)
    cls, pend = {}, None
    for a in toks:
        if a[0]:
            pend = a[0]
        elif pend:
            cls[pend] = a[1] + "/" + a[2]
            pend = None
    return {n: cls.get("CLIENTBOUND_" + n) for n in jar_cb}


def selftest():
    """Every refusal, fired once. A guard nobody has watched fire is not a guard."""
    import contextlib  # noqa: PLC0415
    import io  # noqa: PLC0415

    fails = []

    def expect_die(label, fn):
        buf = io.StringIO()
        try:
            with contextlib.redirect_stderr(buf):
                fn()
        except SystemExit as e:
            if e.code == 2:
                return
            fails.append(f"{label}: exited {e.code}, wanted 2")
            return
        fails.append(f"{label}: did not refuse")

    expect_die("missing registries.json", lambda: registries("/nonexistent/registries.json"))
    expect_die("absent registry key", lambda: reg_names({"a": {"entries": {}}}, "minecraft:block"))
    expect_die("no recordings", lambda: recordings("/nonexistent/r*.json"))
    expect_die("gamerule scrape floor",
               lambda: harness_world(os.devnull))

    # The ceiling guards. Each one exists because an empty or renamed input
    # would otherwise print a ceiling of zero or of the registry size, and both
    # of those read as a measurement.
    expect_die("javap on a class the jar does not have",
               lambda: javap(JAR, "net/minecraft/NoSuchClassHere"))
    expect_die("spawn-egg convention broken", lambda: spawn_egg_types(
        {"minecraft:item": {"entries": {"minecraft:not_a_mob_spawn_egg": {"protocol_id": 0}}},
         "minecraft:entity_type": {"entries": {}}}))
    expect_die("spawn-egg floor", lambda: spawn_egg_types(
        {"minecraft:item": {"entries": {"minecraft:pig_spawn_egg": {"protocol_id": 0}}},
         "minecraft:entity_type": {"entries": {"minecraft:pig": {"protocol_id": 0}}}}))

    class _EmptyGraph:
        calls, news, declares, supers, events = {}, {}, {}, {}, {}

        def subtypes(self):
            return {}

    expect_die("menu scrape floor", lambda: ceiling_menus(_EmptyGraph()))

    # THE HOUSEKEEPING PREMISE GUARD, FIRED THREE WAYS — 2026-08-16. This is
    # the red-when-broken evidence the AGENT-BRIEF's hard rule demands, and it
    # is a reconstruction of the real defect rather than a synthetic one: the
    # first case is `branch trace-chunk-block-entities` applied to the list,
    # verbatim — that branch deletes exactly the `LEVEL_CHUNK_WITH_LIGHT` line
    # from the classification — and before this guard existed the census printed
    # `20` and a `jar` owner straight through it with every test green.
    #
    # PINNED AND SPENT EXPLICITLY, NOT FROM THE MODULE GLOBALS. That branch's
    # own fix is exactly bumping `HOUSEKEEPING_PINNED` and clearing this entry
    # out of `HOUSEKEEPING_SPENT_BY`, so a version of this case that read the
    # live globals would stop dying the moment the fix it exists to prove
    # landed — a check that goes green by having its premise repaired out from
    # under it rather than by anything being tested, which is the exact shape
    # of defect this file's own hard rule refuses everywhere else. The
    # pre-fix snapshot is frozen here instead, so this stays red-when-broken
    # regardless of what the globals say once the real branch is in.
    _stale_pinned = HOUSEKEEPING_PINNED | {"LEVEL_CHUNK_WITH_LIGHT"}
    _stale_spent = {
        **HOUSEKEEPING_SPENT_BY,
        "LEVEL_CHUNK_WITH_LIGHT": "block_entity_type ceiling depends on this staying dropped",
    }
    undropped = _stale_pinned - {"LEVEL_CHUNK_WITH_LIGHT"}
    expect_die("spent drop deleted (branch trace-chunk-block-entities)",
               lambda: ceiling_premise_housekeeping(undropped, spent=_stale_spent, pinned=_stale_pinned))
    expect_die("housekeeping list widened",
               lambda: ceiling_premise_housekeeping(HOUSEKEEPING_PINNED | {"SET_HEALTH"}))
    expect_die("housekeeping list narrowed elsewhere",
               lambda: ceiling_premise_housekeeping(HOUSEKEEPING_PINNED - {"TAB_LIST"}))
    # FIRED A FOURTH WAY — 2026-09-06. The classification stopped being about
    # what is recorded, so the guard had to stop being about that too. This is
    # the case the old shape could not express: the class is unchanged, the
    # packet is still ambient, and somebody has written its decoder arm — which
    # is the edit that actually takes the block-entity ceiling from the 19
    # `getUpdatePacket` overriders to all 49 types.
    #
    # Frozen on the same pre-fix snapshot as the first case, and for the same
    # reason: on main `LEVEL_CHUNK_WITH_LIGHT` has its arm and is spent by
    # nothing, so read from the live globals this case had nothing to refuse
    # and went red as "did not refuse" — a guard proven only while its
    # premise is still broken.
    expect_die("a spent drop acquired a decoder arm",
               lambda: ceiling_premise_housekeeping(
                   _stale_pinned, spent=_stale_spent, pinned=_stale_pinned,
                   undecoded=_stale_pinned - {"LEVEL_CHUNK_WITH_LIGHT"}))
    # ...and stays green on the list as it stands, or the three above would be
    # a guard that refuses everything, which is the other way to be unfalsifiable.
    try:
        cs_mod = load_surface()
        ceiling_premise_housekeeping(
            cs_mod.trace_sets()[0], undecoded=cs_mod.undecoded_families()
        )
    except SystemExit:
        fails.append("housekeeping premise: REFUSED the tree's own classification — "
                     "HOUSEKEEPING_PINNED is stale, or trace.rs moved without it")
    # Every packet a ceiling rule spends must actually be in the pinned list,
    # or the rule is written on a premise nothing has ever asserted.
    for p in HOUSEKEEPING_SPENT_BY:
        if p not in HOUSEKEEPING_PINNED:
            fails.append(f"HOUSEKEEPING_SPENT_BY names {p}, which HOUSEKEEPING_PINNED does not")

    # THE FOLD ITSELF MUST MOVE. The guard above refuses a changed list, but a
    # refusal is only worth having if the number behind it would really have
    # been different — otherwise the whole premise was decorative. Same two
    # channels, same sets, and only the recorder's two decisions varied.
    _chan = {"packet": ("BLOCK_ENTITY_DATA", {f"t{i}" for i in range(20)}),
             "chunk": ("LEVEL_CHUNK_WITH_LIGHT", {f"t{i}" for i in range(49)})}
    _hk = {"LEVEL_CHUNK_WITH_LIGHT"}
    _dec = {"BLOCK_ENTITY_DATA": "block_entity", "LEVEL_CHUNK_WITH_LIGHT": "chunk"}
    for label, hk, dec, want_n, want_owner in (
        ("as the tree stands: chunk dropped", _hk, _dec, 20, "ours"),
        ("un-dropped AND decoded", set(), _dec, 49, "jar"),
        ("un-dropped but still not decoded", set(), {"BLOCK_ENTITY_DATA": "block_entity"}, 20, "ours"),
    ):
        got, van, owner, _l, _d = live_ceiling(_chan, hk, dec)
        if (len(got), owner, len(van)) != (want_n, want_owner, 49):
            fails.append(f"live_ceiling ({label}): got {len(got)}/{owner} of {len(van)},"
                         f" wanted {want_n}/{want_owner} of 49")

    # The chunk channel's own guard, fired on a class whose <init> takes no
    # LevelChunk. `javap` refuses first in a tree with no jar, which is the
    # same allowance the guards above are written under.
    if os.path.exists(JAR):
        expect_die("chunk channel anchor moved",
                   lambda: chunk_payload_carries_every_block_entity(JAR, cls=BE_DATA_PACKET))
        expect_die("chunk channel grew a getUpdatePacket filter",
                   lambda: chunk_payload_carries_every_block_entity(JAR, filter_cls=BE_CLASS))
        if chunk_payload_carries_every_block_entity(JAR) is not True:
            fails.append("chunk channel guard: refused the pinned jar's real classes")
    if camel_to_snake("copperGolem") != "copper_golem":
        fails.append("camel_to_snake: copperGolem")
    if camel_to_snake("warden") != "warden":
        fails.append("camel_to_snake: warden")
    if not MOB_SUFFIX.match("zombieNautilusActivityUpdate"):
        fails.append("MOB_SUFFIX: zombieNautilusActivityUpdate")
    bare = MOB_SUFFIX.match("Brain")
    if bare is None or bare.group(1) != "":
        # A crash here would still be a non-zero exit, but a self-test that
        # reports which guard failed is the difference between a red somebody
        # can act on and a traceback somebody re-runs.
        fails.append("MOB_SUFFIX: bare 'Brain' must yield an empty stem, which the caller drops")
    # `scenario_vocabulary`'s two refusals and its one arithmetic claim, on a
    # checks directory built here rather than on the real corpus — a guard
    # tested against `ci/checks/gameplay` passes for as long as that directory
    # happens to be well-formed, which is not a test of the guard.
    import tempfile  # noqa: PLC0415

    regs = {"minecraft:item": {"entries": {"minecraft:stone": {"protocol_id": 1},
                                           "minecraft:stick": {"protocol_id": 2}}}}
    with tempfile.TemporaryDirectory() as d:
        expect_die("vocabulary: empty checks dir", lambda: scenario_vocabulary(regs, d))

        with open(os.path.join(d, "a.toml"), "w") as f:
            f.write('name = "a"\n[[step]]\ndo = "hold"\nslot = 0\nitem = "minecraft:stone"\n')
        with open(os.path.join(d, "b.toml"), "w") as f:
            # A `hold` in `setup` counts exactly as one in `step` — both put the
            # item in front of vanilla — and a duplicate must not double-count.
            f.write('name = "b"\n[[setup]]\ndo = "hold"\nslot = 0\nitem = "minecraft:stick"\n'
                    '[[step]]\ndo = "hold"\nslot = 1\nitem = "minecraft:stone"\n'
                    '[[step]]\ndo = "place"\nslot = 0\n')
        got = scenario_vocabulary(regs, d)
        if got != {"minecraft:stone", "minecraft:stick"}:
            fails.append(f"vocabulary: wanted stone+stick across setup and step, got {sorted(got)}")

        with open(os.path.join(d, "c.toml"), "w") as f:
            f.write('name = "c"\n[[step]]\ndo = "hold"\nslot = 0\nitem = "minecraft:nosuchitem"\n')
        expect_die("vocabulary: item not in the registry", lambda: scenario_vocabulary(regs, d))

    # **The census's own opaque tally, fired on a synthetic recording.**
    #
    # `trace::read_item_stack` writes `opaque_component`; this script counts
    # nodes carrying it as `item_opaque`. The Rust side proves the field is
    # emitted and that it stops being emitted if `Step::Hold` goes back to
    # writing no components
    # (`the_opaque_arm_is_reachable_through_hold_and_only_because_of_components`).
    # This proves the field is not then dropped on the floor here.
    #
    # Both halves are needed. `0 of 539` in PARITY-ROADMAP 38 had two possible
    # explanations — a harness that could not produce a component-bearing
    # stack, or a counter that could not see one — and 44.5 asserts the first.
    # An untested counter leaves the second standing.
    regs2 = {k: {"entries": {}} for k in (
        "minecraft:sound_event", "minecraft:block_entity_type",
        "minecraft:menu", "minecraft:block", "minecraft:item")}
    regs2["minecraft:entity_type"] = {"entries": {"minecraft:pig": {"protocol_id": 7}}}
    # `registry_reach` refuses a recording with no entities or no blocks, on
    # the ground that the field names must have moved. That floor is a real
    # one and the fixture satisfies it rather than being exempted from it.
    floor_obs = [
        {"kind": "add_entity", "fields": {"kind_id": 7}},
        {"kind": "block_update", "fields": {"state_id": 1, "state": "minecraft:stone"}},
    ]
    opaque_stack = {"count": 1, "item_id": 909, "item": "minecraft:diamond_sword",
                    "comp_added": 1, "comp_removed": 0, "opaque_component": 3}
    plain_stack = {"count": 1, "item_id": 1, "item": "minecraft:stone"}
    with tempfile.TemporaryDirectory() as d:
        rp = os.path.join(d, "r1.json")
        with open(rp, "w") as fh:
            json.dump({"scenarios": [{"obs": floor_obs + [
                {"kind": "container_set_slot", "fields": {"item": opaque_stack}},
                {"kind": "container_set_slot", "fields": {"item": plain_stack}},
            ]}]}, fh)
        _s, _u, _sc, t = registry_reach([rp], regs2)
        if t["item_opaque"] != 1:
            fails.append(f"item_opaque: wanted 1 for one opaque stack, got {t['item_opaque']}")
        if t["item_bearing"] != 2:
            fails.append(f"item_bearing: wanted 2 stack-bearing observations, got {t['item_bearing']}")

        # The control: the same recording with the marker removed must count
        # zero, or the tally is counting stacks rather than opaque ones.
        rp2 = os.path.join(d, "r2.json")
        bare = dict(opaque_stack)
        del bare["opaque_component"]
        with open(rp2, "w") as fh:
            json.dump({"scenarios": [{"obs": floor_obs + [
                {"kind": "container_set_slot", "fields": {"item": bare}},
                {"kind": "container_set_slot", "fields": {"item": plain_stack}},
            ]}]}, fh)
        _s, _u, _sc, t2 = registry_reach([rp2], regs2)
        if t2["item_opaque"] != 0:
            fails.append(f"item_opaque control: wanted 0 with no marker, got {t2['item_opaque']}")

    for f in fails:
        print(f"selftest FAIL {f}", file=sys.stderr)
    print("selftest: " + ("GREEN" if not fails else f"{len(fails)} FAILED"))
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
