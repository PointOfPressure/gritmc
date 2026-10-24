#!/usr/bin/env python3
"""attribute-residual.py — put every residual position on a structure, or on none.

    ci/attribute-residual.py <positions.txt> <starts.txt> [margin]

`positions.txt` is a list of `x y z` lines — `ENS_OUT/res-cum.txt` from
`ci/floor-ensemble.sh`, or the `--positions` output of `region-differ diff`
passed through the same anchored filter.

`starts.txt` is `worldgen-dump jigsaw-starts <vanilla-region-dir>` — *vanilla's*
saved starts, not ours. Attributing our residual to our own boxes would score a
structure we never placed as "outside every structure box", which is precisely
the reading that has to be avoided: the two savanna villages were 13,659 blocks
of residual while this port placed no village at all.

`margin` (default 16) is added to every side of a start's union box. A
structure's blocks are not confined to the union of its piece boxes — a village
street re-grades terrain outside them, and the beard reaches further still.

# Why this exists as a file

`docs/WIDE-WINDOW-R16-ALL-DIMENSIONS.md` records the correction this replaces:
a caveat written from a *sample* of the disagreement list said "~73% of that
residual is a single trial chamber" when the truth was three chambers and
91.8%. A sampled attribution under-counts a family with more than one cluster,
and nothing catches it until every position is attributed. So this attributes
every position, and prints the unattributed remainder as its own row rather
than dropping it.

# What it deliberately does not do

It does not decide *why* a position inside a box is wrong. A block inside a
village box may be a wrong village block, a right village block on wrong
terrain, or terrain the beard should have raised and did not. This narrows the
suspect list; it does not close it.
"""

import collections
import re
import sys


def read_starts(path, margin):
    """[(id, chunk_x, chunk_z, (x0,y0,z0,x1,y1,z1))] — one union box per start."""
    starts = []
    cur = None
    head = re.compile(r"^START (\S+) chunk \((-?\d+), (-?\d+)\)")
    # Both listing shapes: the jigsaw one and the template one (`end_city`).
    box = re.compile(r"BB=\[(-?\d+) (-?\d+) (-?\d+) \.\. (-?\d+) (-?\d+) (-?\d+)\]")
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            m = head.match(line)
            if m:
                cur = [m.group(1), int(m.group(2)), int(m.group(3)), None]
                starts.append(cur)
                continue
            m = box.search(line)
            if m and cur is not None:
                b = [int(g) for g in m.groups()]
                if cur[3] is None:
                    cur[3] = b
                else:
                    u = cur[3]
                    cur[3] = [
                        min(u[0], b[0]), min(u[1], b[1]), min(u[2], b[2]),
                        max(u[3], b[3]), max(u[4], b[4]), max(u[5], b[5]),
                    ]
    out = []
    for sid, cx, cz, b in starts:
        if b is None:
            # A start with no piece boxes cannot claim a position. Refusing
            # loudly: a zero-size box silently claims nothing and looks the
            # same as a start that was simply not read.
            print(f"  WARNING: start {sid} at ({cx},{cz}) has no piece boxes", file=sys.stderr)
            continue
        out.append((sid, cx, cz, (
            b[0] - margin, b[1] - margin, b[2] - margin,
            b[3] + margin, b[4] + margin, b[5] + margin,
        )))
    return out


def main():
    if len(sys.argv) < 3:
        print(__doc__.strip().splitlines()[2].strip(), file=sys.stderr)
        return 2
    positions, starts_path = sys.argv[1], sys.argv[2]
    margin = int(sys.argv[3]) if len(sys.argv) > 3 else 16

    starts = read_starts(starts_path, margin)
    if not starts:
        # Rule: a measurement that cleared every candidate is suspect. With no
        # boxes at all every position lands in "outside", which reads as a
        # clean terrain result and would be pure artefact.
        print("REFUSING: the starts listing yielded no boxes", file=sys.stderr)
        return 2

    counts = collections.Counter()
    total = 0
    with open(positions, encoding="utf-8") as handle:
        for line in handle:
            parts = line.split()
            if len(parts) < 3:
                continue
            try:
                x, y, z = int(parts[0]), int(parts[1]), int(parts[2])
            except ValueError:
                continue
            total += 1
            hit = None
            for sid, cx, cz, b in starts:
                if b[0] <= x <= b[3] and b[1] <= y <= b[4] and b[2] <= z <= b[5]:
                    hit = f"{sid.replace('minecraft:', '')} ({cx},{cz})"
                    break
            counts[hit or "outside every structure box"] += 1

    print(f"ATTRIBUTION  {total} positions, {len(starts)} starts, margin {margin}")
    for name, n in counts.most_common():
        share = 100.0 * n / total if total else 0.0
        print(f"  {n:8d}  {share:5.1f}%  {name}")
    # A position can sit in two boxes; first match wins, so overlaps are
    # charged to whichever start the listing named first. Said out loud
    # because a silent tie-break is how an attribution flatters one family.
    print("  (a position inside two boxes is charged to the first start listed)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
