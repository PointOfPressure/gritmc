#!/usr/bin/env python3
"""Where vanilla varies run to run and this server does not — a divergence no check can see.

    ci/determinism-gap.py <recording-dir> [<recording-dir> ...]

Each directory holds `vanilla.json` and `rustmc.json`. Any tree's
`oracle-work/gameplay` will do, and the point is to pass **many**: this
measures a distribution, so one pair says nothing at all. It reads recordings
that already exist — no server, no build, no lock.

# The class of divergence this exists to find

The corpus compares **one** run of vanilla against **one** run of this server.
That instrument is blind, by construction, to a whole family:

> vanilla's behaviour is a distribution, ours is a constant inside it.

Every individual comparison passes, because our constant is a value vanilla
does produce. Nothing is ever red. And the two servers are not the same
server: one of them is making a choice the other has hard-coded.

`ci/corpus-floor.sh` asks the neighbouring question — what vanilla cannot
reproduce about itself — and stops there, because its job is to keep an
unstable field from being read as a divergence. It never goes on to ask
whether *we* reproduce that same field **too well**. That is the gap, and it is
the exact mirror of the one this repository keeps finding: an instrument that
cannot fail in one direction.

# What it found, and why it replaced a wrong version of itself

`551-attacking-from-out-of-range` walks a player from z=20 to z=3 and the
observer hears their step sounds. Read on one pair, this looked like a plain
divergence — vanilla four, ours five — and it was reported as one. **It is
not.** Across every recording on this box:

    vanilla   4 in 32 runs, 5 in 39 runs   (n = 71)
    ours      5 in 66 runs, 4 in 0         (n = 66)

Vanilla is very close to a coin flip and we are pinned. So a per-run check
comparing the two would flap on vanilla's own variance and be declared
unstable, which is the correct treatment of the *reference* and the wrong
conclusion about the *subject*. The first version of this claim, made from a
single pair, said "we emit one too many" and was wrong in a way only more
recordings could show.

This one is counted on the observer stream and filtered to the step sounds by
volume, so it is not subject to the caveat below in the way the `block_update`
rows are — but it is still a count over a whole scenario and it is still a
screen. What it establishes is that a difference exists in the distributions;
what it does not establish is that the mechanism is the walk rather than the
window.

# IT COUNTS RAW OBSERVATIONS, NOT PROJECTED ONES, AND THAT IS ITS BIGGEST HOLE

Every count here is over a scenario's whole `obs` list: both clients, every
timestamp, no `within` bound. The projection applies three filters this does
not — `observe_from`, `near`, and the `observe` list — and **all three exist
because scenarios leak into one another.** They share one world and one pair
of sessions, and `compare::near`'s own header records `080`'s mob still
rotating its head inside `090`'s window 32 blocks away.

So vanilla "varying" here can be vanilla varying, or it can be a neighbouring
scenario landing in the read differently from run to run. **The two button rows
below are the second thing**, checked by hand: vanilla's extra `block_update`s
are `minecraft:air` at `at_ms = 0` — before the step ran — and at ~1490 ms,
which is leakage of exactly the shape `near` was written to bound. They are not
a determinism gap and they are left in the output rather than special-cased,
because a tool that quietly drops the rows its author could not explain is
worse than one that prints them with this paragraph attached.

**Confirming a row therefore means re-reading it with the projection applied.**
This is a screen. It is the third on this branch, and the third to produce hits
that dissolved on a second look — which is itself the finding: the projection's
filters are load-bearing, and reading recordings without them manufactures
divergences as readily as applying them hides ones.

# Reading the output

`vanilla spread` is the set of counts vanilla produced; `ours` likewise. A row
appears when vanilla produced more than one distinct value and we produced
exactly one. Small n is stated rather than hidden: two recordings cannot
establish that anything is a distribution, and the header says how many were
read.
"""

import collections
import json
import os
import sys


def counts(path, side):
    """{(scenario, kind): n} for one recording, or None if it cannot be read."""
    p = os.path.join(path, f"{side}.json")
    if not os.path.exists(p):
        return None
    try:
        with open(p, encoding="utf-8") as fh:
            doc = json.load(fh)
    except (OSError, ValueError):
        return None
    out = collections.Counter()
    for sc in doc.get("scenarios", []):
        for o in sc.get("obs", []):
            out[(sc["name"], o["kind"])] += 1
    return out


def main():
    dirs = sys.argv[1:]
    if len(dirs) < 3:
        # A refusal, not a small answer. Two pairs cannot show that anything is
        # a distribution, and a table drawn from them would be read as one.
        sys.exit(
            "determinism-gap: give at least three recording directories — this "
            "measures a distribution and cannot be run on one pair.\n"
            "  try: ci/determinism-gap.py ~/rustmc*/oracle-work/gameplay"
        )

    spread = {"vanilla": collections.defaultdict(collections.Counter),
              "rustmc": collections.defaultdict(collections.Counter)}
    n = {"vanilla": 0, "rustmc": 0}
    for d in dirs:
        for side in ("vanilla", "rustmc"):
            c = counts(d, side)
            if c is None:
                continue
            n[side] += 1
            for key, v in c.items():
                spread[side][key][v] += 1

    if not n["vanilla"] or not n["rustmc"]:
        sys.exit(f"determinism-gap: read {n['vanilla']} vanilla and {n['rustmc']} subject "
                 "recordings — need both sides")

    rows = []
    for key, vdist in spread["vanilla"].items():
        odist = spread["rustmc"].get(key)
        if not odist:
            continue
        # Vanilla makes a choice; we do not. The `sum(...) >= 3` guard keeps a
        # field seen twice from being called a distribution.
        if len(vdist) > 1 and len(odist) == 1 and sum(odist.values()) >= 3:
            only = next(iter(odist))
            rows.append((key[0], key[1], dict(sorted(vdist.items())), only, sum(odist.values())))

    rows.sort(key=lambda r: (-len(r[2]), r[0], r[1]))
    print(
        f"determinism-gap: {n['vanilla']} vanilla and {n['rustmc']} subject recordings\n"
        f"{len(rows)} (scenario, kind) pairs where vanilla varies and this server does not\n"
        "  a constant inside vanilla's distribution passes every single-run comparison\n"
    )
    for scenario, kind, vdist, only, on in rows:
        v = ", ".join(f"{k} in {c}" for k, c in vdist.items())
        print(f"  {scenario[:52]:<54} {kind:<24}")
        print(f"      vanilla: {v}      ours: {only} in {on}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
