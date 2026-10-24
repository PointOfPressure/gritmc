#!/usr/bin/env python3
"""A per-block-kind census across N region directories, joined into one table.

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/family-census.py --bbox -1063 -679 -1049 -665 \
#         vanA=~/owcensus-refs/van-...-open-a \
#         vanB=~/owcensus-refs/van-...-open-b \
#         before=~/palecensus-out/owA-before/region \
#         after=~/palecensus-out/owA-after/region \
#         --match pale_oak --match pale_moss --match dark_oak
#
# prints one row per block kind: every column's count, the twin floor
# `|vanA - vanB|`, and the absolute error `|vanA - ours|` for every column
# after the first two. The first directory named is VANILLA, and it is the
# one every error is measured against.
#
#     --match <substr>   restrict to kinds containing this; repeatable.
#                        Omitted, every kind that is non-zero somewhere.
#     --min <n>          drop rows where no column reaches n (default 1)
#     --total            print the summed absolute error over the rows shown
#     --columns          also count distinct (x, z) COLUMNS per kind, and
#                        blocks per column. **Requires `--match`**: it reads
#                        positions rather than counts, and an unfiltered
#                        position dump is every block in the window. That is
#                        not a slow command, it is an unbounded one — it was
#                        written without the filter once and had to be killed.
#
# **`--columns` is the one that changed a conclusion.** `pale_oak_log` reads
# 2744 against vanilla's 2397 and looks like trees that are too big. By column
# it is 621 against 552 with logs per column only 1.8% high: seventeen trees
# too many, each very slightly tall. Those want different fixes, and the block
# count alone points at the wrong one. A trunk is 2x2 columns, a patch is a
# footprint, a vein is a face — pick the unit the feature is built in.
#
# WHAT BREAKS IF YOU IGNORE THIS: the two things this exists to stop.
#
# **A count delta is not a measurement until its floor is beside it.** Two
# vanilla runs of the same window do not agree block for block — ORACLE rule 6
# — so `pale_moss_block 2836 vs 723` is only a defect if vanilla's own two
# runs agree about `pale_moss_block` much more closely than that. This prints
# the twin's disagreement in the same row and NEVER subtracts it, exactly as
# every other floor in this project is carried.
#
# **A per-kind census does not adjudicate placement.** A kind whose count is
# exactly right can still be in the wrong places; `region-differ diff` is what
# answers that. This tool answers "how much of this block does each side
# have", which is the question a port that unblocks a refusal is asking, and
# it is the question window A's biome residual cannot answer for a biome that
# sits under its floor.
# ===== end interface; everything below is why =====
"""

# # WHY THIS IS NOT `ci/block-kind-census.sh`

# That script set-differences POSITIONS for one kind across two dirs, which is
# what an attribution to a structure needs and it is the right tool for
# "obsidian -26, lava +68 — is that one defect or two". It is the wrong tool
# here for two reasons: it takes one `ours` directory, so a before/after is two
# invocations and a manual join, and it prints positions, so a family of a
# dozen kinds is a dozen sorted files rather than a table.
#
# The question this one is for is the one §58.6 ruled has to be asked on every
# port that unblocks a refusal: **read the block census beside the residual**.
# That reading is a table of counts, over a family, with the before and the
# after in adjacent columns, because the finding is usually the direction of
# one row rather than the value of any of them.

# # WHY THE FIRST DIRECTORY IS SPECIAL AND THE SECOND IS TOO

# Argument order is the semantics, as it is in `region-differ`: swap the first
# two and every floor is unchanged but every error is measured against the
# other vanilla run. That is legitimate and it is a different table, so it has
# to be a deliberate act rather than an accident of typing.
#
# The floor uses exactly two vanilla columns because that is the N these
# windows were harvested at. A third reference would raise the floor, never
# lower it — `docs/DETERMINISM.md` §4 — so a floor printed here is a LOWER
# BOUND on vanilla's self-disagreement for that kind, which is the safe
# direction: it can only make a defect look larger than it is provable to be.

import argparse
import os
import re
import subprocess
import sys

DIFFER = os.environ.get("REGION_DIFFER", "./target/release/region-differ")


def census(path, bbox, matches=()):
    """`region-differ census` for one dir.

    -> ({block name: count}, {block name: distinct (x, z) columns})

    The column map is empty unless `matches` is non-empty. Each match is a
    SEPARATE `--positions` pass, pushed down into `region-differ` rather than
    filtered here: the prefilter is what bounds the output, and without it the
    dump is every block in the bbox.
    """
    if os.path.isdir(os.path.join(path, "region")):
        path = os.path.join(path, "region")
    base = [DIFFER, "census", path]
    if bbox:
        base += ["--bbox"] + [str(v) for v in bbox]

    out = subprocess.run(base, capture_output=True, text=True, check=False)
    if out.returncode != 0:
        sys.exit(f"region-differ census failed on {path}:\n{out.stderr[-2000:]}")
    counts = {}
    for line in out.stdout.splitlines():
        m = re.match(r"\s*(\d+)\s+(\S+)\s*$", line)
        if m:
            counts[m.group(2)] = int(m.group(1))
    if not counts:
        sys.exit(f"census of {path} produced no rows — is it a region dir?")

    columns = {}
    for match in matches:
        got = subprocess.run(
            base + ["--match", match, "--positions"],
            capture_output=True,
            text=True,
            check=False,
        )
        if got.returncode != 0:
            sys.exit(f"region-differ positions failed on {path}:\n{got.stderr[-2000:]}")
        for line in got.stdout.splitlines():
            parts = line.split()
            if len(parts) == 4:
                columns.setdefault(parts[3], set()).add((parts[0], parts[2]))
    return counts, {k: len(v) for k, v in columns.items()}


def main():
    ap = argparse.ArgumentParser(add_help=True)
    ap.add_argument("--bbox", nargs=4, type=int, metavar=("X1", "Z1", "X2", "Z2"))
    ap.add_argument("--match", action="append", default=[])
    ap.add_argument("--min", type=int, default=1)
    ap.add_argument("--total", action="store_true")
    ap.add_argument("--columns", action="store_true")
    ap.add_argument("cols", nargs="+", metavar="NAME=DIR")
    a = ap.parse_args()

    cols = []
    for spec in a.cols:
        if "=" not in spec:
            sys.exit(f"expected NAME=DIR, got {spec!r}")
        name, _, path = spec.partition("=")
        cols.append((name, os.path.expanduser(path)))
    if len(cols) < 2:
        sys.exit("need at least two columns: the vanilla pair")

    if a.columns and not a.match:
        sys.exit(
            "--columns without --match would dump every block position in the "
            "window. Name the kinds you want."
        )
    raw = [
        (name, census(path, a.bbox, a.match if a.columns else ()))
        for name, path in cols
    ]
    data = [(name, counts) for name, (counts, _) in raw]
    colcounts = {name: cols_ for name, (_, cols_) in raw}
    van_a, van_b = data[0][1], data[1][1]

    kinds = set()
    for _, c in data:
        kinds |= set(c)
    if a.match:
        kinds = {k for k in kinds if any(m in k for m in a.match)}
    kinds = {k for k in kinds if max(c.get(k, 0) for _, c in data) >= a.min}

    ours = data[2:]
    w = max([len(k) for k in kinds] + [16])
    head = f"{'block':<{w}}" + "".join(f"{n:>12}" for n, _ in data)
    head += f"{'floor':>10}" + "".join(f"{'err ' + n:>12}" for n, _ in ours)
    print(head)
    print("-" * len(head))

    totals = {n: 0 for n, _ in ours}
    for k in sorted(kinds, key=lambda k: -van_a.get(k, 0)):
        row = f"{k:<{w}}" + "".join(f"{c.get(k, 0):>12}" for _, c in data)
        row += f"{abs(van_a.get(k, 0) - van_b.get(k, 0)):>10}"
        for n, c in ours:
            e = abs(van_a.get(k, 0) - c.get(k, 0))
            totals[n] += e
            row += f"{e:>12}"
        print(row)
        if a.columns:
            cells = []
            for n, c in data:
                nc = colcounts[n].get(k, 0)
                per = (c.get(k, 0) / nc) if nc else 0.0
                cells.append(f"{nc}@{per:.2f}")
            print(f"{'  cols@per':<{w}}" + "".join(f"{cell:>12}" for cell in cells))

    if a.total:
        print("-" * len(head))
        pad = w + 12 * len(data) + 10
        floor = sum(abs(van_a.get(k, 0) - van_b.get(k, 0)) for k in kinds)
        print(
            f"{'absolute error':<{w}}"
            + " " * (12 * len(data))
            + f"{floor:>10}"
            + "".join(f"{totals[n]:>12}" for n, _ in ours)
        )
        print(f"{'(rows shown)':<{w}}{len(kinds):>12}")


if __name__ == "__main__":
    main()
