#!/usr/bin/env bash
# light-radius-sweep.sh — how much world a chunk's light depends on.
#
# # The question this answers, and why it is not optional
#
# `ci/t2-gameplay.sh` runs at `--view-distance 4`. The chunk-send lane found
# that its own defect — Chebyshev versus `ChunkPos.distanceSquared` ordering —
# is **invisible below radius 4**, because the two metrics are identical out to
# radius 3. That was not predictable from the port; it was measured. Light has
# no reason to share that threshold and every reason to have one of its own,
# so this measures it rather than assuming the 3x3 grid `ci/light-golden.sh`
# uses is wide enough.
#
# The method is ORACLE rule 14: widen the context until the answer stops
# changing, and quote the converged one. It regenerates the golden at
# radius 0..N and asks, per case, the smallest radius at which the **centre
# chunk's** wire bytes stop moving.
#
# A defect confined to a chunk at distance d from the chunk being observed
# cannot reach that chunk's packet unless d is inside that radius — so the
# converged number is also the answer to "how much world must a scenario load
# before a light defect can show up in the packet it is looking at".
#
# **A case that converges at 0 is a case that grades nothing about
# cross-chunk propagation**, and this prints that verdict rather than leaving
# it to be inferred: a suite made only of such cases would be green against a
# port with no horizontal propagation at all.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

MAXR="${1:-4}"
for r in $(seq 0 "$MAXR"); do
    echo "== generating at radius $r" >&2
    LIGHT_RADIUS="$r" "$ROOT/ci/light-golden.sh" "$WORK/r$r.json" >/dev/null
done

python3 - "$WORK" "$MAXR" <<'PY'
import json
import sys

work, maxr = sys.argv[1], int(sys.argv[2])

FIELDS = ("sky_mask", "block_mask", "empty_sky_mask", "empty_block_mask",
          "sky", "block")


def centre(case):
    """The chunk at (0, 0) — the only one present at every radius."""
    for ch in case["chunks"]:
        if ch["x"] == 0 and ch["z"] == 0:
            return tuple(json.dumps(ch[f]) for f in FIELDS)
    raise SystemExit("no centre chunk in case " + case["name"])


runs = {}
for r in range(maxr + 1):
    with open(f"{work}/r{r}.json") as f:
        runs[r] = {c["name"]: centre(c) for c in json.load(f)["cases"]}

names = list(runs[maxr])
print(f"{'case':12} {'converges at':>12}   {'differs from r=%d at radius' % maxr}")
worst = 0
for name in names:
    ref = runs[maxr][name]
    differing = [r for r in range(maxr + 1) if runs[r][name] != ref]
    conv = (max(differing) + 1) if differing else 0
    worst = max(worst, conv)
    note = "" if differing else "   <- grades no cross-chunk propagation"
    print(f"{name:12} {conv:>12}   {differing if differing else 'none'}{note}")

print()
print(f"converged context radius over all cases: {worst}")
if worst >= maxr:
    raise SystemExit(
        f"NOT CONVERGED: some case still moves at r={maxr}. Re-run with a "
        f"larger maximum — quoting {worst} would be quoting the edge of the "
        f"sweep, not a converged answer.")
PY
