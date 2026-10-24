#!/usr/bin/env python3
"""Positional census of the order-stable core against the savanna villages'
`feature_pool_element` positions.

Input:
  census/core145.tsv        the 145 order-stable charged positions, `x_y_z<TAB>vanilla<TAB>ours`
  the two committed golden piece vectors, which are VANILLA'S OWN saved NBT

Question the brief asks, in this order:
  1. which pool elements, 2. how many positions each,
  3. does each position actually fall inside that element's footprint.

A `feature_pool_element`'s bounding box is a single block — `Vec3i.ZERO` size —
so "footprint" cannot mean the piece box. It means the extent of the feature
the element plants. This script therefore attributes by nearest element and
reports the horizontal/vertical offsets, so the footprint claim is measured
rather than assumed.
"""

import json
import sys
from collections import Counter, defaultdict

ROOT = "/home/sayer/grit/work-villagepool"
GOLDEN = [
    ("c4677", f"{ROOT}/crates/rustmc-worldgen/golden/village_savanna_20260722_c4677.json"),
    ("c8177", f"{ROOT}/crates/rustmc-worldgen/golden/village_savanna_20260722_c8177.json"),
]

elements = []
for tag, path in GOLDEN:
    pieces = json.load(open(path))
    if isinstance(pieces, dict):
        pieces = pieces["pieces"]
    for i, p in enumerate(pieces):
        if p["type"] == "minecraft:feature_pool_element":
            elements.append({"village": tag, "index": i, "pos": tuple(p["pos"]),
                             "rot": p["rotation"]})

core = []
for line in open(f"{ROOT}/census/core145.tsv"):
    key, van, ours = line.rstrip("\n").split("\t")
    x, y, z = (int(v) for v in key.split("_"))
    core.append((x, y, z, van, ours))

# Which residual rows are the declared village gap at all? Attribute by block
# kind first, exactly as the parent did, so the two censuses can be compared.
DECLARED = ("acacia_leaves", "acacia_log", "hay_block")


def is_declared(van, ours):
    kind = van.split("[")[0].removeprefix("minecraft:")
    return kind in DECLARED


declared = [r for r in core if is_declared(r[3], r[4])]
other = [r for r in core if not is_declared(r[3], r[4])]

print(f"order-stable core            {len(core)}")
print(f"  declared village kinds     {len(declared)}")
print(f"  everything else            {len(other)}")
print()
print("--- declared rows by kind pair ---")
for (a, b), n in Counter(
    (r[3].removeprefix('minecraft:'), r[4].removeprefix('minecraft:')) for r in declared
).most_common():
    print(f"  {n:4d}  {a} -> {b}")
print()

# Attribution: nearest feature element, with the offsets printed so a claim of
# "inside the footprint" is a measurement.
per_el = defaultdict(list)
unattributed = []
for x, y, z, van, ours in declared:
    best, bestd = None, None
    for e in elements:
        ex, ey, ez = e["pos"]
        d = max(abs(x - ex), abs(z - ez))
        if bestd is None or d < bestd:
            best, bestd = e, d
    if bestd is not None and bestd <= 16:
        per_el[(best["village"], best["index"], best["pos"])].append(
            (x, y, z, van, ours, bestd, y - best["pos"][1])
        )
    else:
        unattributed.append((x, y, z, van, ours, bestd))

print(f"--- {len(elements)} feature elements, ranked by blocks attributed ---")
print(f"{'village':8} {'idx':>4} {'element pos':>18} {'blocks':>7} {'dxz':>5} {'dy range':>10}  kinds")
rows = []
for e in elements:
    key = (e["village"], e["index"], e["pos"])
    hits = per_el.get(key, [])
    if hits:
        dxz = max(h[5] for h in hits)
        dys = [h[6] for h in hits]
        kinds = Counter(h[3].split("[")[0].removeprefix("minecraft:") for h in hits)
        kd = " ".join(f"{k}:{v}" for k, v in kinds.most_common())
    else:
        dxz, dys, kd = 0, [0], "-"
    rows.append((len(hits), e, dxz, dys, kd))

for n, e, dxz, dys, kd in sorted(rows, key=lambda r: -r[0]):
    print(f"{e['village']:8} {e['index']:4d} {str(e['pos']):>18} {n:7d} {dxz:5d} "
          f"{min(dys):4d}..{max(dys):<4d}  {kd}")

print()
print(f"attributed   {sum(len(v) for v in per_el.values())}")
print(f"unattributed {len(unattributed)}")
for u in unattributed:
    print("   ", u)
print()
print(f"elements with zero blocks: {sum(1 for n, *_ in rows if n == 0)} of {len(elements)}")
