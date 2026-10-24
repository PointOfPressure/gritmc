#!/usr/bin/env python3
"""How far a scenario's entity leaks into later scenarios, and how much of that
the spatial bound can even see.

# ===== AGENT INTERFACE =====
#
#     ./ci/entity-leak-audit.py                 the box's vanilla recordings
#     ./ci/entity-leak-audit.py a.json b.json   named recordings
#
# Prints. Never gates. Reads raw traces of ONE server at a time.
# ===== end interface; everything below is why =====

# THE HAZARD, WHICH IS ALREADY WRITTEN DOWN, AND THE MITIGATION, WHICH IS NOT
# WHAT IT SAYS IT IS

A corpus run is ONE server and ONE session. `ci/checks/gameplay/README.md` is
explicit about that for counters — `ServerPlayer.nextContainerCounter` runs for
the life of the player — and `ci/checks/projection-drops.toml`'s
`project.spatial_bound` row is explicit about it for entities:

    "`080` spawns a mob, never removes it, goes on rotating its head, and seven
     of its packets landed in `090`'s strict projection 32 blocks away"

The mitigation named there is `within`: `compare::near` drops an observation
whose position falls outside the check's radius. That row also records, in its
own residual, that the filter **fails open** — `place` returns `None` for an
observation it cannot locate and `near` takes its "nobody ever told me where
this is" branch.

**This script measures how much of the actual leak the filter can place, and
over 61 recordings the answer is none of it.** Not "most of it gets through":
**0 of 4,496** leaked observations from a spawned entity carried a position at
all. Every one arrived as `set_entity_motion`, `move_entity` (a delta),
`rotate_head` or `move_entity_heartbeat`, and not one of those four kinds
carries an absolute position for `near` to test. The bound was written for this
hazard and cannot act on any of it.

And the mob is not even the worst of it. Measured 2026-08-18, 61 recordings:

    torch-falls-with-its-support                  1308 obs   0 placeable
    dropping-an-item-throws-it-on-the-floor        912 obs   0 placeable
    spawn-egg-makes-a-mob                          864 obs   0 placeable
    container-click-throw-drops-the-whole-stack    816 obs   0 placeable
    a-dropped-item-goes-into-your-inventory        448 obs   0 placeable
    container-click-throw-drops-one-item           148 obs   0 placeable

**Five of the six are dropped items, not mobs.** The live-mob rule in
`ci/checks/gameplay/README.md` was written for a walking golem and the class it
names is wider than the rule: an item entity does not walk, but it lives for
five minutes, and `EntityType.ITEM.trackDeltas()` is true, so it emits a
`set_entity_motion` every time its velocity changes for the whole of that. The
1308 from `torch-falls-with-its-support` all land in
`coral-dries-out-unprompted`, in 34 of 61 recordings.

The `player` rows are separate and are NOT leaks — the two session participants
are announced once and present by design. They are printed anyway, and it is
the one place the bound bites: `entity_position` carries a position, so 327 of
the player stream's 3,582 are placeable.

That is not an argument for widening `near`. There is nothing to widen: a
delta-encoded move genuinely does not say where it happened, so no radius can
help, and the honest description is that a live entity is bounded by scenario
ORDER and by nothing else. `ci/checks/gameplay/README.md`'s live-mob rule is
therefore the whole mitigation and not the backstop it reads as.

# WHY THIS IS A DISTRIBUTION AND IS REPORTED AS ONE

`spawn-egg-makes-a-mob` leaks 1..108 observations per recording, and does not
leak at all in 14 of 61. It reaches `chest-opens-a-screen` in 40 of 61 and
`redstone-torch-powers-wire` in 17 of 61. A single pair would have shown "seven
packets" — which is what the ledger row records, and it is true of one run and
is not a bound.
the `corpus-projection-audit` branch retracted claim `551` on exactly this shape, so
every number here is printed with its N and its denominator.

# NOT A COMPARISON

One server's own stream at a time. Nothing here is a divergence, which is why
reading it without the projection is sound. The point of the script is
precisely that the projection cannot see this, so applying it would hide the
thing being measured.
"""

from __future__ import annotations

import glob as globmod
import json
import os
import sys
from collections import Counter, defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

DEFAULT = [
    os.path.expanduser("~/rustmc*/oracle-work/gameplay/vanilla.json"),
    os.path.join(ROOT, "oracle-work", "gameplay", "vanilla.json"),
]


def eid(v):
    return v["$entity"] if isinstance(v, dict) and "$entity" in v else None


def names() -> dict[int, str]:
    p = os.path.join(ROOT, "data", "vanilla", "entity_types.json")
    if not os.path.exists(p):
        return {}
    with open(p) as f:
        return {v: k for k, v in json.load(f)["entity_types"].items()}


def main(argv: list[str]) -> int:
    pats = argv or DEFAULT
    byid = names()

    nrec = 0
    # source scenario -> victim scenario -> recordings that leaked
    reach: dict[str, Counter] = defaultdict(Counter)
    kinds: dict[str, Counter] = defaultdict(Counter)
    placeable: dict[str, Counter] = defaultdict(Counter)
    per_rec: dict[str, Counter] = defaultdict(Counter)
    sources: Counter = Counter()

    for pat in pats:
        for p in sorted(globmod.glob(pat)):
            try:
                with open(p) as f:
                    d = json.load(f)
                scen = d["scenarios"]
            except Exception:
                continue
            nrec += 1
            # Every entity announced by an `add_entity`, and where it was
            # announced. A leak is that id appearing in a LATER scenario.
            born: dict[int, tuple[int, str, int]] = {}
            ran: set[str] = {sc["name"] for sc in scen}
            for i, sc in enumerate(scen):
                for o in sc.get("obs", []):
                    if o.get("kind") != "add_entity":
                        continue
                    e = eid(o.get("fields", {}).get("entity"))
                    k = o.get("fields", {}).get("kind_id")
                    if e is not None and e not in born:
                        born[e] = (i, sc["name"], k)
            per_src_this_rec: dict[str, int] = defaultdict(int)
            # Which victims this recording leaked into, per source. Collected
            # per RECORDING and folded in once at the end: a session has two
            # players, and counting reach per entity read `122/61`.
            reach_this_rec: dict[str, set] = defaultdict(set)
            for e, (i, src, k) in born.items():
                # A PLAYER is not a leak. The two session participants are
                # announced once and are present for every scenario by design,
                # which is the corpus working rather than bleeding — and the
                # first draft of this script counted them, produced a 3,582-obs
                # "leak" from `530`, and would have been a finding about
                # nothing. They are still counted, under `player`, because the
                # mechanism is identical and a reader who does not see them
                # will wonder whether they were checked.
                bucket = "player" if byid.get(k) == "minecraft:player" else "spawned"
                key = (src, bucket)
                sources[key] += 0
                n = 0
                for j in range(i + 1, len(scen)):
                    hits = 0
                    for o in scen[j].get("obs", []):
                        f = o.get("fields", {})
                        hit = any(eid(v) == e for v in f.values())
                        ents = f.get("entities")
                        if not hit and isinstance(ents, list):
                            hit = any(eid(x) == e for x in ents)
                        if not hit:
                            continue
                        hits += 1
                        kinds[key][o["kind"]] += 1
                        if "at" in f:
                            placeable[key][o["kind"]] += 1
                    if hits:
                        reach_this_rec[key].add(scen[j]["name"])
                        n += hits
                per_src_this_rec[key] += n
                if n:
                    sources[key] += n
            # One row per (source scenario, bucket) per RECORDING, so the
            # denominator is recordings and not entities. Counting entities
            # gave `122 of 122` out of 64 files.
            for key, victims in reach_this_rec.items():
                for v in victims:
                    reach[key][v] += 1
            for key in {(src, b) for (_, (_, src, kk)) in born.items()
                        for b in ["player" if byid.get(kk) == "minecraft:player"
                                  else "spawned"]}:
                per_rec[key][per_src_this_rec.get(key, 0)] += 1
            _ = ran

    print(f"N = {nrec} vanilla recording(s). One server's own stream at a time "
          f"— a presence census, not a comparison.\n")

    if not sources:
        print("no entity announced in one scenario was named in a later one.")
        return 0

    for key in sorted(sources, key=lambda s: -sources[s]):
        src, bucket = key
        tot = sum(kinds[key].values())
        if not tot:
            continue
        pl = sum(placeable[key].values())
        clean = per_rec[key].get(0, 0)
        seen = sum(per_rec[key].values())
        tag = "" if bucket == "spawned" else "   [player — a session participant, not a leak]"
        print(f"=== {src}{tag} ===")
        print(f"  leaked into a later scenario in {seen - clean} of {seen} "
              f"recordings that ran it")
        print(f"  {tot} leaked observation(s) in total; "
              f"{pl} carry an absolute position "
              f"({100 * pl / tot:.1f}%) and are therefore the only ones "
              f"`compare::near` can act on")
        for k, v in kinds[key].most_common():
            print(f"    {k:24s} {v:6d}   placeable {placeable[key][k]}")
        print("  reached:")
        for victim, c in reach[key].most_common()[:12]:
            print(f"    {victim:52s} in {c}/{seen} recordings")
        if len(reach[key]) > 12:
            print(f"    ... and {len(reach[key]) - 12} further scenario(s)")
        rng = sorted(n for n in per_rec[key] if n)
        if rng:
            print(f"  per-recording leak count: 0 in {clean} of {seen}, "
                  f"otherwise {rng[0]}..{rng[-1]}")
        print()

    print("`placeable 0` means the spatial bound cannot exclude the leak — "
          "`compare::near` fails open on an observation it cannot locate, and "
          "a delta-encoded move never says where it happened. Scenario ORDER "
          "is the whole mitigation; see ci/checks/gameplay/README.md.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
