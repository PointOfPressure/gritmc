#!/usr/bin/env python3
"""Attribute a corpus run's declared count to individual checks.

    ci/verdict-diff.py <report.json> [--baseline ci/checks/gameplay-baseline.toml]

`ci/t2-gameplay.sh` prints a total and refuses a regression. What it does not
print is **which** checks account for a change, and the standing rule here is
that a rise in the declared count has to be attributable check by check.

That attribution was done twice with a throwaway script called `verdicts.py`,
which no longer exists — so the numbers it produced cannot be re-derived by
anyone reading the commits that quote them. **A number in a tracked document
has to come from a tracked script.** This is that script.

It prints, against the declared floor:

  * every check **above** its floor, which is a candidate to lock in;
  * every check **below** it, which is a regression the gate has already
    refused;
  * every check the baseline does not list at all;
  * the seven-way verdict census, and whether it sums to the corpus size.

That last line is not decoration. `matched + diverged + absent + vacuous +
mute + inert + errored == total` is the arithmetic that has caught a
miscounted denominator here before, and **0 vacuous / 0 mute / 0 inert /
0 error is a protected property** — a check that has quietly become
unfalsifiable shows up there and nowhere else.

`mute` is the third of the broken-check verdicts and the newest: the measured
step, the last `[[step]]` and the one the file is named for, contributed
nothing on either side. It is separate from `inert` on purpose, because the
two send a reader to different places — `inert` says the whole window was
forced, `mute` says the window was fine and the assertion was not in it.

Exit is always 0: this reports, it does not gate. `ci/t2-gameplay.sh` is the
gate.
"""

from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    print("verdict-diff: needs python 3.11 for tomllib", file=sys.stderr)
    sys.exit(2)

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BASELINE = ROOT / "ci" / "checks" / "gameplay-baseline.toml"

# Worst to best. `Outcome`'s own ordering in `compare.rs` is the authority for
# what counts as a regression; this mirrors it for reporting only.
RANK = {
    "error": 0,
    "vacuous": 1,
    "mute": 2,
    "inert": 3,
    "absent": 4,
    "diverge": 5,
    "match": 6,
}


def main(argv: list[str]) -> int:
    args = argv[1:]
    baseline_path = DEFAULT_BASELINE
    if "--baseline" in args:
        i = args.index("--baseline")
        baseline_path = Path(args[i + 1])
        del args[i : i + 2]
    if len(args) != 1:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2

    report = json.loads(Path(args[0]).read_text())
    outcomes = report["outcomes"]
    baseline = tomllib.loads(baseline_path.read_text()).get("expect", {})

    census = Counter(o["verdict"] for o in outcomes)
    strict_census = Counter(o["strict"] for o in outcomes)

    better, worse, unlisted = [], [], []
    for o in sorted(outcomes, key=lambda o: o["name"]):
        want = baseline.get(o["name"])
        if want is None:
            unlisted.append(o)
            continue
        got = o["verdict"]
        if RANK.get(got, -1) > RANK.get(want, -1):
            better.append((o["name"], want, got))
        elif RANK.get(got, -1) < RANK.get(want, -1):
            worse.append((o["name"], want, got, o.get("first_difference")))

    print(f"== {len(outcomes)} checks against {baseline_path.name}")
    for name, want, got in better:
        print(f"  ABOVE FLOOR  {name}: floor {want}, measured {got}")
    for name, want, got, why in worse:
        print(f"  BELOW FLOOR  {name}: floor {want}, measured {got}")
        if why:
            print(f"               {why}")
    for o in unlisted:
        print(f"  UNLISTED     {o['name']}: measured {o['verdict']}")
    if not (better or worse or unlisted):
        print("  every check is exactly at its declared floor")

    total = len(outcomes)
    summed = sum(census.values())
    order = ["match", "diverge", "absent", "vacuous", "mute", "inert", "error"]
    line = "  ".join(f"{k} {census.get(k, 0)}" for k in order)
    print(f"== declared  {census.get('match', 0)}/{total}   {line}")
    print(
        f"   arithmetic {summed} == {total}"
        + ("" if summed == total else "   *** MISMATCH ***")
    )
    protected = {k: census.get(k, 0) for k in ("vacuous", "mute", "inert", "error")}
    bad = {k: v for k, v in protected.items() if v}
    print(
        "   protected  0 vacuous / 0 mute / 0 inert / 0 error"
        + ("  HELD" if not bad else f"  *** BROKEN: {bad} ***")
    )
    # Reported, never a parity claim, and never from one run. See
    # `gameplay-baseline.toml`'s closing block.
    print(
        f"== strict    {strict_census.get('match', 0)}/{total}   "
        "(diagnostic only — one run, vanilla's own nondeterminism inside it, "
        "never quotable as parity)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
