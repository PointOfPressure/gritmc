#!/usr/bin/env python3
"""Which of the jar's entity types this corpus can reach, and which it can GRADE.

# ===== AGENT INTERFACE =====
#
#     ./ci/entity-reach.py                     the tables
#     ./ci/entity-reach.py --recordings        add the measured census
#     ./ci/entity-reach.py --check             refuse if the ledger has drifted
#
# ===== end interface; everything below is why =====

# THE QUESTION THIS ANSWERS, AND THE ONE IT REFUSES TO

`ci/corpus-surface.py` gave the corpus a denominator for *packets*. This is the
same move for *entities*, and it exists because every parity claim this project
publishes is about blocks. Worldgen, block states, block entities, redstone,
containers. Entities were measured incidentally and never counted.

Counting them the obvious way produces a number that is worse than no number:
`4 of 158` reads as 2.5% of a work queue, and it is not a work queue. Two
thirds of the jar's entity types cannot appear on a creative superflat with
`difficulty=peaceful`, `spawn-radius=0` and two players, and calling a ghast a
gap is arithmetic rather than information — the same trap
`ci/corpus-surface.py`'s header names for boss-bar packets.

So this file publishes **three** numbers and insists on the third:

  * **reached**   — a `kind_id` some recording's `add_entity` actually carried.
    Measured, not declared. This is the only one that is a fact about the
    world rather than about the tree.
  * **declared**  — an entity type a check in `ci/checks/gameplay/` puts in the
    world on purpose. A declared type that never gets reached is a check that
    is not doing what it says.
  * **gradeable** — declared, AND arriving with a rotation the server *chose*
    rather than *drew*. This is the number the area is about, and it is much
    smaller than either of the others.

# WHY `gradeable` IS A SEPARATE COLUMN AND NOT A REFINEMENT OF `reached`

`ClientboundAddEntityPacket.write` ends `xRot`, `yRot`, `yHeadRot`, `data`
(`tools/oracle-runner/src/trace.rs` names all four). Whether those bytes can be
compared at all is decided by the **route** that put the entity in the world,
not by the entity's type — and there are exactly two routes in this jar that
matter here. Both are read from the pinned jar with
`javap -p -c -constants`, on 2026-08-18:

    net/minecraft/world/entity/EntityType.class
      T create(ServerLevel, PostSpawnProcessor<T>, BlockPos, EntitySpawnReason,
               boolean, boolean)
        103  invokeinterface RandomSource.nextFloat:()F
        111  invokestatic    Mth.wrapDegrees:(F)F
        115  invokevirtual   Entity.snapTo:(DDDFF)V
        168  invokevirtual   Mob.finalizeSpawn:(...)

    net/minecraft/world/level/block/CarvedPumpkinBlock.class
      private static void spawnGolemInWorld(Level, BlockPattern$BlockPatternMatch,
                                            Entity, BlockPos)
          2  invokestatic    clearPatternBlocks:(...)
         33  fconst_0
         34  fconst_0
         35  invokevirtual   Entity.snapTo:(DDDFF)V     <- a literal zero
         40  invokevirtual   Level.addFreshEntity:(...)

The first draws the yaw from an unseeded `RandomSource`, so **every entity type
reachable by a spawn egg is ungradeable on `add_entity` by construction** —
not because our server is wrong but because vanilla cannot reproduce itself.
`080-spawn-egg-makes-a-mob` is the measurement: 25 flaps in 25 comparisons over
N = 26 cold vanilla servers, and `ci/entity-spawn-probe.sh` at N = 6 saw six
distinct yaws in six runs. The second passes a literal `0.0F` and the same
probe saw byte-identical payloads across all six.

**That is why the egg count is published as a warning and not as a queue.** 88
of the jar's 158 entity types have a spawn egg, and all 88 resolve to a type by
name, so 88 types are one `hold` and one `place` from being in a recording.
Reaching them would add 88 checks to `ci/checks/gameplay-unstable.toml` and
nothing to the parity figure. An agent reading `4 of 158` and writing the
obvious eighty-eight scenarios would make the corpus worse, slower and no more
informative, and would not find that out for a week.

# WHAT `unclassified` MEANS HERE, WHICH IS NOTHING

`docs/PARITY-ROADMAP.md` §36 is the precedent and it is blunt: 80 sections were
carried as `unclassified` under a 33% headline and the word meant *no
information at all*, and classified with an instrument that could produce a
positive the same 120 sections read completely differently. So the remainder
row below is printed as a count with the word `unclassified` attached and is
never summed into a percentage. It is not "unreachable". Nothing here has asked.
"""

from __future__ import annotations

import argparse
import glob as globmod
import json
import os
import sys
import tomllib
from collections import Counter, defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHECKS = os.path.join(ROOT, "ci", "checks", "gameplay")
LEDGER = os.path.join(ROOT, "ci", "checks", "entity-reach.toml")

# Where the jar-derived tables live. `data/vanilla` is generated
# (`ci/gen-gameplay-tables.sh`) and is not in the tree, so a worktree that has
# never generated them must be told that rather than given a wrong number.
DATA = os.path.join(ROOT, "data", "vanilla")


class Missing(Exception):
    pass


def load_json(name: str):
    p = os.path.join(DATA, name)
    if not os.path.exists(p):
        raise Missing(
            f"{p} absent. These tables are jar-derived and generated: run "
            f"./ci/gen-gameplay-tables.sh. This script refuses rather than "
            f"reporting a census over a registry it could not read."
        )
    with open(p) as f:
        return json.load(f)


def entity_types() -> dict[str, int]:
    return load_json("entity_types.json")["entity_types"]


def items() -> set[str]:
    return set(load_json("items.json")["items"])


def read_ledger() -> dict:
    with open(LEDGER, "rb") as f:
        return tomllib.load(f)


def scenarios() -> dict[str, dict]:
    out = {}
    for p in sorted(globmod.glob(os.path.join(CHECKS, "*.toml"))):
        with open(p, "rb") as f:
            d = tomllib.load(f)
        d["_file"] = os.path.basename(p)
        out[d["name"]] = d
    return out


def measured(patterns: list[str]) -> tuple[int, Counter, dict[int, set[str]]]:
    """Distinct `add_entity.kind_id` over recordings, with the scenarios.

    RAW STREAMS, NO PROJECTION. That is deliberate and it is safe *for this
    question only*: this counts what one server announced, which is a presence
    census, not a comparison. `docs/cold-start/corpus-projection-audit.md` is
    about the other kind — reading recordings without the projection to look
    for *divergences* manufactures them — and none of this function's output
    is a divergence. Nothing here compares two streams.
    """
    seen: Counter = Counter()
    where: dict[int, set[str]] = defaultdict(set)
    n = 0
    for pat in patterns:
        for p in sorted(globmod.glob(pat)):
            try:
                with open(p) as f:
                    d = json.load(f)
                scen = d["scenarios"]
            except Exception:
                continue
            n += 1
            for sc in scen:
                for o in sc.get("obs", []):
                    if o.get("kind") != "add_entity":
                        continue
                    k = o.get("fields", {}).get("kind_id")
                    if k is not None:
                        seen[k] += 1
                        where[k].add(sc["name"])
    return n, seen, where


DEFAULT_RECORDINGS = [
    os.path.expanduser("~/rustmc*/oracle-work/gameplay/vanilla.json"),
    os.path.join(ROOT, "oracle-work", "gameplay", "vanilla.json"),
]


def main() -> int:
    ap = argparse.ArgumentParser(add_help=True)
    ap.add_argument("--recordings", nargs="*", default=None,
                    help="glob(s) of vanilla recordings for the measured census; "
                         "bare flag uses the box's usual locations")
    ap.add_argument("--check", action="store_true",
                    help="refuse if the ledger disagrees with the check files")
    a = ap.parse_args()

    try:
        ents = entity_types()
        its = items()
    except Missing as e:
        print(f"entity-reach: {e}", file=sys.stderr)
        return 2

    byid = {v: k for k, v in ents.items()}
    led = read_ledger()
    scen = scenarios()

    rows = led.get("spawns", [])
    declared: dict[str, list[str]] = defaultdict(list)
    problems: list[str] = []

    RESIDUE = {"session", "resolves", "persists", "drifts", "walks"}
    for r in rows:
        t, ck = r["type"], r["check"]
        # A row with no `residue` is a check whose effect on the checks after
        # it has never been stated. That is the second thing this ledger is
        # for and it is required, not optional.
        if r.get("residue") not in RESIDUE:
            problems.append(
                f"NO RESIDUE     {ck} does not say what it leaves in the world "
                f"(one of {', '.join(sorted(RESIDUE))})"
            )
        elif not r.get("residue_why", "").strip():
            problems.append(f"BARE RESIDUE   {ck} declares `residue` with no reason")
        if t not in ents:
            problems.append(f"UNKNOWN TYPE   {t} (row for {ck}) is not in the jar's registry")
        if ck not in scen:
            problems.append(f"VANISHED CHECK {ck} (row for {t}) is not in ci/checks/gameplay/")
        declared[t].append(ck)

    # The reverse direction, which is the one that catches a check being added.
    # A check that puts an entity in the world and is not in this ledger is the
    # failure mode `ci/checks/projection-drops.toml` exists to prevent for
    # fields: a loss nobody wrote down.
    named = {r["check"] for r in rows}
    for name, d in scen.items():
        if "add_entity" in d.get("observe", []) and name not in named:
            problems.append(
                f"UNDECLARED     {d['_file']} observes add_entity and names no "
                f"entity type in ci/checks/entity-reach.toml"
            )

    eggs = {"minecraft:" + i.split(":")[1][: -len("_spawn_egg")]
            for i in its if i.endswith("_spawn_egg")}
    eggs &= set(ents)

    gradeable = {r["type"] for r in rows if r.get("rotation") == "chosen"}
    drawn = {r["type"] for r in rows if r.get("rotation") == "drawn"}

    print(f"jar entity types                              {len(ents)}")
    print(f"declared by a check in ci/checks/gameplay/    {len(declared)}")
    print(f"  of those, rotation CHOSEN (gradeable)       {len(gradeable)}")
    print(f"  of those, rotation DRAWN  (ungradeable)     {len(drawn)}")
    print(f"reachable by a spawn egg, one hold + one place {len(eggs)}"
          f"   <- NOT a work queue, see this file's header")
    print(f"unclassified remainder                        "
          f"{len(ents) - len(set(declared) | eggs)}   (unclassified means unknown)")
    unbounded = [r for r in rows if r.get("residue") in ("drifts", "walks")]
    print(f"checks leaving an UNBOUNDED entity behind      {len(unbounded)}"
          f"   <- `compare::near` filters none of it")
    print()
    if unbounded:
        print("UNBOUNDED RESIDUE — these are facts about the checks that run after them:")
        for r in sorted(unbounded, key=lambda r: r["check"]):
            print(f"  {r['residue']:9s} {r['check']}")
        print()

    print("DECLARED, by route:")
    for r in sorted(rows, key=lambda r: (r["route"], r["type"])):
        mark = "gradeable" if r.get("rotation") == "chosen" else "DRAWN    "
        print(f"  {r['type']:28s} {mark}  {r['route']:22s} {r['check']}")

    if a.recordings is not None:
        pats = a.recordings or DEFAULT_RECORDINGS
        n, seen, where = measured(pats)
        print()
        print(f"MEASURED over {n} vanilla recording(s) — raw streams, no projection")
        print(f"  distinct kind_id ever carried by add_entity: {len(seen)}")
        for k, v in seen.most_common():
            print(f"    {byid.get(k, '?'):28s} id={k:<5} {v:6d} obs   "
                  f"{', '.join(sorted(where[k]))}")
        never = sorted(t for t in declared if ents[t] not in seen)
        if never:
            print(f"  declared but never measured ({len(never)}): "
                  f"{', '.join(t.split(':')[1] for t in never)}")
            print("    A declared type with no measurement is not yet evidence of "
                  "anything. It is either a check that has never run or a check "
                  "that does not do what it says, and this tool cannot tell you "
                  "which — run the corpus.")

    if problems:
        print()
        for p in problems:
            print(f"  {p}")
        if a.check:
            print(f"\nentity-reach: {len(problems)} problem(s)", file=sys.stderr)
            return 1
    elif a.check:
        print(f"\nentity-reach: {len(rows)} declared spawn(s) still as tagged")
    return 0


if __name__ == "__main__":
    sys.exit(main())
