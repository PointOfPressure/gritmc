#!/usr/bin/env python3
"""order-geometry.py — what shape is the order-sensitive set?

The brief behind this asked to *characterise* the dependence before explaining
it: which positions move when our chunk-visitation order changes, and do they
have a geometry — chunk borders, region borders, a radius from the window edge,
a height band.  `docs/DETERMINISM.md` §11 ran the same census on **vanilla's**
floor and got a nearly flat border profile (1.12x at the chunk edge against a
uniform scatter), which is what says vanilla's order noise is *whole features
landing elsewhere* rather than two chunks contesting the blocks between them.
This prints the identical census for our order-moved set so the two can be put
side by side.

Also printed, and it is the cheap discriminator: **how many of the eight orders
move each position.** A position every order moves is genuinely contested by two
chunks whose features overlap it; one that a single order moves is a rare pair
of chunks that only that order separates.  A defect that is really one bug shows
up as a mass at one end.

usage:
  order-geometry.py --orderdir <dir> [--orders zx,...] [--charge <file>]
                    [--window 48 48 80 80]

`--charge` is a file of `x y z ...` lines to restrict to — e.g. the admissible
charge — so the census can be run on the whole moved set and on the charged
part of it and the two compared.
"""

from __future__ import annotations

import os
import sys


def die(msg: str) -> int:
    print(f"order-geometry: {msg}", file=sys.stderr)
    return 2


def census(name: str, pts, degree=None) -> None:
    """pts: iterable of (x, y, z)."""
    border: dict[int, int] = {}
    yband: dict[int, int] = {}
    chunkdist: dict[int, int] = {}
    regionedge = 0
    total = 0
    for x, y, z in pts:
        total += 1
        lx, lz = x & 15, z & 15
        d = min(lx, 15 - lx, lz, 15 - lz)
        border[d] = border.get(d, 0) + 1
        b = (y >> 4) << 4
        yband[b] = yband.get(b, 0) + 1
        cx, cz = x >> 4, z >> 4
        # distance in chunks from the window edge, window is 48..80
        e = min(cx - 48, 80 - cx, cz - 48, 80 - cz)
        chunkdist[e] = chunkdist.get(e, 0) + 1
        if cx % 32 in (0, 31) or cz % 32 in (0, 31):
            regionedge += 1
    if total == 0:
        print(f"{name}: empty")
        return
    print(f"{name}: {total:,}")
    # A 16x16 chunk has 60 cells at d=0, 52 at 1, 44, 36, 28, 20, 12, 4 at d=7.
    cells = {d: (60 - 8 * d) for d in range(8)}
    print("  distance from chunk border")
    print("    d  cells  uniform   observed   excess")
    for d in range(8):
        u = cells[d] / 256.0
        o = border.get(d, 0) / total
        print(f"    {d}   {cells[d]:>4}  {u * 100:6.2f}%  {border.get(d, 0):>9,}"
              f" {o * 100:6.2f}%  {o / u:5.2f}x")
    print("  y bands (16)")
    for b in sorted(yband):
        if yband[b] * 200 >= total:  # >= 0.5%
            print(f"    {b:>5}: {yband[b]:>9,}  {100.0 * yband[b] / total:5.1f}%")
    print("  distance in chunks from the window edge")
    for e in sorted(chunkdist)[:6]:
        print(f"    {e:>2}: {chunkdist[e]:>9,}  {100.0 * chunkdist[e] / total:5.1f}%")
    # Two of the 33 chunk columns in 48..80 are region-file edges (63 and 64),
    # and likewise two rows, so a position is on one with probability
    # 1 - (31/33)^2 = 11.75% under a uniform scatter. Printed because a raw
    # 12.8% reads as an enrichment until the baseline is beside it.
    print(f"  on a region-file edge chunk: {regionedge:,}"
          f"  {100.0 * regionedge / total:.1f}%   (uniform: "
          f"{100.0 * (1 - (31 / 33.0) ** 2):.2f}%)")
    if degree is not None:
        print("  how many of the eight orders move it")
        hist: dict[int, int] = {}
        for v in degree.values():
            hist[v] = hist.get(v, 0) + 1
        for k in sorted(hist):
            print(f"    {k}: {hist[k]:>9,}  {100.0 * hist[k] / max(len(degree), 1):5.1f}%")
    print()


def main(argv: list[str]) -> int:
    def opt(name, default=None):
        if name in argv:
            return argv[argv.index(name) + 1]
        return default

    orderdir = opt("--orderdir")
    if not orderdir:
        return die("need --orderdir")
    orders = opt("--orders", "zx,zx_rev,zx_zrev,reverse,spiral,spiral_in,spawn,diag").split(",")
    chargef = opt("--charge")

    degree: dict[tuple[int, int, int], int] = {}
    for o in orders:
        p = os.path.join(orderdir, f"default-vs-{o}.pos")
        if not os.path.exists(p):
            return die(f"missing {p}")
        with open(p) as fh:
            for line in fh:
                f = line.split()
                if len(f) < 5:
                    continue
                try:
                    k = (int(f[0]), int(f[1]), int(f[2]))
                except ValueError:
                    continue
                degree[k] = degree.get(k, 0) + 1

    census("ORDER-MOVED  any of the eight orders changes our block",
           degree.keys(), degree)

    if chargef:
        keep = set()
        with open(chargef) as fh:
            for line in fh:
                f = line.split()
                if len(f) < 3:
                    continue
                try:
                    keep.add((int(f[0]), int(f[1]), int(f[2])))
                except ValueError:
                    pass
        sub = {k: v for k, v in degree.items() if k in keep}
        census("CHARGED AND ORDER-MOVED", sub.keys(), sub)
        census("CHARGED AND ORDER-STABLE", (k for k in keep if k not in degree))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
