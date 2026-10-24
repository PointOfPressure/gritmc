#!/usr/bin/env python3
"""Is `add_entity.movement` reproducible? Per entity type, over N recordings.

# ===== AGENT INTERFACE =====
#
#     ./ci/entity-movement-report.py r1.json r2.json ...      (N >= 2)
#     ENTITY_RUNS=6 ./ci/entity-spawn-probe.sh                takes them
#
# Exit 0 always. This prints; it does not gate. See "NOT A GATE" below.
# ===== end interface; everything below is why =====

# WHAT QUESTION THIS ANSWERS

`ci/checks/projection-drops.toml` classifies `add_entity.movement` as `armed` —
real server behaviour, compared by nothing — and gives one reason for the drop:
`ServerEntity.getLastSentMovement()` is a tick-phase coin toss for a standing
player and four unseeded `nextFloat()` draws for a thrown item.

**That reason covers two of the eleven entity types this corpus reaches.** It
says nothing about a `falling_block`, a `copper_golem`, an `item_frame`, a
`minecart` or a `pig`. Until 2026-08-18 the question could not even be asked:
`trace::decode` consumed the three packed shorts, so the field was in no
recording anywhere and no instrument could see it. Naming it is what makes this
script possible, and this script is the only reason naming it was worth doing.

# THE THREE OUTCOMES, AND WHY `constant` IS NOT `reproducible`

Per `(scenario, kind_id)`:

  * **drawn**       — more than one distinct value across the runs. Settled.
    Whatever the servers do, no comparison of this field can pass, and a check
    that compared it would be a check that must fail. One counterexample is
    enough, so this verdict is as strong at N = 3 as at N = 26.
  * **constant**    — one distinct value across all N. NOT the same claim.
    A value that varies less often than the runs can see reads identical, and
    the resolving power is printed beside every row precisely so that
    `constant` cannot be quoted as `reproducible`. `docs/PARITY-ROADMAP.md` §13
    is the standing lesson: for the End, two references agreeing gave a floor
    of 0 and six gave 252.
  * **absent**      — the scenario produced no `add_entity` in some run. Not a
    verdict about the field at all, and reported separately so it cannot be
    counted as agreement. It is usually the scenario, not the field.

# THE ASYMMETRY IS THE POINT AND IT IS THE SAFE DIRECTION

`drawn` justifies *widening* the projection's exemption and is conclusive.
`constant` justifies *narrowing* it and is not — it is a lower bound that grows
with N, and narrowing on it would trade a field that is dropped for no reason
for a check that flaps for no reason. So the honest use of this output is:

    drawn    -> the drop is justified FOR THAT TYPE, write it down
    constant -> a candidate for narrowing, with its N and its p_hide attached,
                and never a narrowing on its own

# NOT A GATE, AND NOT A COMPARISON

Nothing here reads two servers. Every input is one server's own recordings, so
nothing this prints is a divergence and nothing can be a parity claim. It reads
raw traces without the projection, which is safe for exactly this reason and
would not be if it were comparing anything —
`docs/cold-start/corpus-projection-audit.md` is about the other kind, where
three screens run without the projection produced hits that all dissolved.
"""

from __future__ import annotations

import json
import os
import sys
from collections import defaultdict


def entity_names() -> dict[int, str]:
    """`kind_id` -> name, or an empty map in a tree with no generated tables.

    The fallback is a real path, not a defensive one: this script is run
    straight after `ci/entity-spawn-probe.sh`, in a tree that may not have
    generated `data/vanilla` yet, and refusing there would throw away six cold
    vanilla runs over a lookup table used only for pretty-printing. So the
    caller must format the result as a STRING whatever it got back — the first
    version interpolated `names.get(k, k)` with `:26s`, which is a `ValueError`
    the moment the table is absent and the id falls through as an `int`. That
    crashed after twenty minutes of recording.
    """
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    p = os.path.join(root, "data", "vanilla", "entity_types.json")
    if not os.path.exists(p):
        return {}
    with open(p) as f:
        return {v: k for k, v in json.load(f)["entity_types"].items()}


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: entity-movement-report.py r1.json r2.json ...  (N >= 2)",
              file=sys.stderr)
        return 2

    names = entity_names()
    runs = []
    for p in argv:
        with open(p) as f:
            runs.append(json.load(f))
    n = len(runs)

    # (scenario, kind_id) -> {run index -> movement value as a stable string}
    vals: dict[tuple[str, int], dict[int, str]] = defaultdict(dict)
    missing_field: set[tuple[str, int]] = set()

    for i, r in enumerate(runs):
        for sc in r["scenarios"]:
            for o in sc.get("obs", []):
                if o.get("kind") != "add_entity":
                    continue
                f = o.get("fields", {})
                k = f.get("kind_id")
                if k is None:
                    continue
                if "movement" not in f:
                    missing_field.add((sc["name"], k))
                    continue
                # Several spawns of one type in one scenario are joined, so a
                # scenario spawning two items is one row and not two that
                # silently disagree with each other.
                prev = vals[(sc["name"], k)].get(i, "")
                vals[(sc["name"], k)][i] = prev + json.dumps(f["movement"]) + ";"

    if missing_field:
        print(f"{len(missing_field)} (scenario, kind_id) pair(s) carry an "
              f"`add_entity` with NO `movement` field.")
        print("  These recordings predate the field being named "
              "(recorder before 2026-08-18). This script cannot say anything "
              "about them and does not try — the bytes were consumed by the "
              "decoder and are not in the file to be read.")
        for s, k in sorted(missing_field):
            print(f"    {names.get(k, str(k))}  {s}")
        print()

    # p_hide: the chance a value that really varies looked constant here. A
    # field flapping with probability q is invisible when every one of the N
    # runs draws the same value; the worst case a run of N identical values
    # cannot exclude at alpha = 0.05 is q with (1-q)^(N-1) = 0.05.
    alpha = 0.05
    p_hide = 1.0 - alpha ** (1.0 / (n - 1)) if n > 1 else 1.0

    print(f"N = {n} recording(s) of ONE server. Raw traces, no projection — a "
          f"reproducibility census, not a comparison.")
    print(f"`constant` at this N cannot exclude a field that varies less often "
          f"than {p_hide:.1%} of the time.\n")

    drawn, constant, partial = [], [], []
    for (s, k), per_run in sorted(vals.items()):
        if len(per_run) < n:
            partial.append((s, k, len(per_run)))
            continue
        distinct = sorted(set(per_run.values()))
        (drawn if len(distinct) > 1 else constant).append((s, k, distinct))

    print(f"DRAWN — more than one value across {n} runs. Conclusive: no "
          f"comparison of this field can pass for these.")
    if not drawn:
        print("  (none)")
    for s, k, d in drawn:
        print(f"  {names.get(k, str(k)):26s} {len(d)} distinct   {s}")
        for v in d[:3]:
            print(f"      {v[:110]}")
    print()

    print(f"CONSTANT — one value in all {n}. A candidate for narrowing the "
          f"drop, and NOT a narrowing on its own.")
    if not constant:
        print("  (none)")
    for s, k, d in constant:
        print(f"  {names.get(k, str(k)):26s} {d[0][:90]}   {s}")
    print()

    if partial:
        print("ABSENT IN SOME RUN — not a verdict about the field.")
        for s, k, seen in partial:
            print(f"  {names.get(k, str(k)):26s} in {seen}/{n} runs   {s}")
        print()

    print(f"drawn {len(drawn)}, constant {len(constant)}, partial {len(partial)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
