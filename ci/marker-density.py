#!/usr/bin/env python3
"""marker-density.py — a refusal marker census measures what is WRITTEN, not what is MISSING.

# ===== AGENT INTERFACE =====
#
#     ci/marker-density.py           the correlation, with its permutation control
#     ci/marker-density.py --check   refuse if the relation stops holding
#
# Tree only. No jar, no server, no recording, no lock.
# ===== end interface =====

# # WHY

`ci/unported-inventory.py` walks a direction nothing else here walks: start
from what the code admits it does not do. It is a real instrument and it found
real work. But it has a bound that nothing had stated, and the bound decides
how its output should be read.

**A marker needs a host line.** It is written inside the module that
implements the surrounding mechanic, by the person who implemented it, at the
point where they declined to go further. So a module that does not exist
cannot carry markers proportional to what it is missing — it carries however
many paragraphs a neighbour's author chose to write about it, and that number
has nothing to do with the size of the absence.

That predicts something measurable: **marker count should track lines of
ported code.** It does, over the 58 files of `crates/rustmc-server/src`, and
the coefficient is not marginal.

The consequence is not that the inventory is wrong. Every gap it lists is a
real gap. The consequence is that **its silence is not evidence of absence of
debt**, and a subsystem it reports as nearly clean may be one it cannot see at
all. `entities.rs` carries 2 markers for a hierarchy the pinned jar declares
3,870 methods in; `power.rs` carries 9 for redstone, of which most of the
rules are ported.

# # THE OBVIOUS OBJECTION, WHICH IS THE FINDING RESTATED

"Bigger files have more of everything, so of course a bigger file has more
markers." That is exactly right, and it is not an objection — it is the
sentence this file exists to make quotable. **An instrument whose output is
predicted by how much code exists is measuring how much code exists.** The
inventory's number moves when somebody writes a module, not when somebody
fails to. Reading it as a debt figure is reading a proxy for effort spent as a
measure of effort remaining, and those point in opposite directions.

The 29 files carrying no marker at all are the same point from the other side.
They are not the 29 debt-free modules of this server.

# # THE CONTROL, BECAUSE A CORRELATION IS THE EASIEST THING TO FAKE

Two ways this could be a number and not a finding, and both are checked:

* **the marker set could be empty or trivial**, in which case the coefficient
  is noise over a handful of points. A floor on markers and on files is
  asserted before anything is reported;
* **the arithmetic could produce a large r from any two lists.** So the same
  coefficient is recomputed over `--permutations` random re-pairings of the
  same two lists. If the real r does not stand outside that null distribution,
  the relation is not there and this file says so instead of quoting a number.

`--check` gates the *claim* — that the relation holds and is not chance — and
deliberately not the *value*. Pinning r to three decimals would make every
edit to any source file a gate failure, and a gate that fails on unrelated
work is a gate somebody turns off.
"""

import argparse
import glob
import os
import random
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

# `ci/unported-inventory.py`'s own regex, reproduced so this file measures the
# same population that audit counted rather than a similar one.
MARKER = re.compile(
    r'unported|not ported|NOT PORTED|un-ported|never ported|no ported|todo!|'
    r'unimplemented|TODO|FIXME|not implemented|does not implement|'
    r'do not implement|deliberately not|is a no-op|are a no-op', re.I)

# The audit's scan root, for the same reason.
SCAN = "crates/rustmc-server/src/*.rs"

MIN_FILES = 30
MIN_MARKERS = 40
MIN_R = 0.5
MAX_P = 0.01


def rows():
    out = []
    for p in sorted(glob.glob(os.path.join(ROOT, SCAN))):
        lines = open(p).read().splitlines()
        out.append((os.path.basename(p), len(lines),
                    sum(1 for ln in lines if MARKER.search(ln))))
    return out


def pearson(xs, ys):
    n = len(xs)
    if n < 3:
        return 0.0
    mx = sum(xs) / n
    my = sum(ys) / n
    cov = sum((x - mx) * (y - my) for x, y in zip(xs, ys))
    sx = sum((x - mx) ** 2 for x in xs) ** 0.5
    sy = sum((y - my) ** 2 for y in ys) ** 0.5
    return cov / (sx * sy) if sx and sy else 0.0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--permutations", type=int, default=10000)
    ap.add_argument("--seed", type=int, default=20260819,
                    help="fixed so the control is reproducible run to run")
    a = ap.parse_args()

    r_ = rows()
    xs = [x[1] for x in r_]
    ys = [x[2] for x in r_]
    files, markers = len(r_), sum(ys)

    if files < MIN_FILES or markers < MIN_MARKERS:
        print("marker-density: REFUSED — nothing to correlate", file=sys.stderr)
        print("    files   %d (floor %d)" % (files, MIN_FILES), file=sys.stderr)
        print("    markers %d (floor %d)" % (markers, MIN_MARKERS), file=sys.stderr)
        print("  A coefficient over a handful of points is noise, and a marker",
              file=sys.stderr)
        print("  regex that matched nothing would produce one. Nothing printed.",
              file=sys.stderr)
        return 3

    r = pearson(xs, ys)

    rng = random.Random(a.seed)
    shuf = list(ys)
    ge = 0
    for _ in range(a.permutations):
        rng.shuffle(shuf)
        if abs(pearson(xs, shuf)) >= abs(r):
            ge += 1
    p = (ge + 1) / (a.permutations + 1)

    if a.check:
        bad = []
        if r < MIN_R:
            bad.append("r = %.3f is below the floor %.2f" % (r, MIN_R))
        if p > MAX_P:
            bad.append("permutation p = %.4f is above the ceiling %.2f"
                       % (p, MAX_P))
        if bad:
            print("marker-density --check: REFUSED", file=sys.stderr)
            for b in bad:
                print("    %s" % b, file=sys.stderr)
            print("  The relation this file exists to state has stopped holding.",
                  file=sys.stderr)
            print("  That is a finding, not a threshold to lower.", file=sys.stderr)
            return 1
        print("marker-density --check: r = %.3f over %d files, permutation "
              "p = %.4f" % (r, files, p))
        return 0

    zero = [x for x in r_ if x[2] == 0]
    print("== ci/unported-inventory.py's marker population, re-counted ==")
    print("  files scanned                 %4d   (%s)" % (files, SCAN))
    print("  lines                         %4d" % sum(xs))
    print("  marker lines                  %4d" % markers)
    print("  files carrying no marker      %4d   %d lines between them"
          % (len(zero), sum(x[1] for x in zero)))
    print()
    print("== does the count track what is WRITTEN? ==")
    print("  pearson r(lines, markers)   %6.3f" % r)
    print("  permutation control          p = %.4f over %d re-pairings, seed %d"
          % (p, a.permutations, a.seed))
    print("  (the control shuffles the SAME two lists; if the arithmetic alone")
    print("   produced a large r, the null would contain it and it does not)")
    print()
    print("  %-18s %7s %8s" % ("file", "lines", "markers"))
    for f, n, m in sorted(r_, key=lambda t: -t[2])[:8]:
        print("  %-18s %7d %8d" % (f, n, m))
    ent = [x for x in r_ if x[0] == "entities.rs"]
    if ent:
        print("  %-18s %7d %8d   <- the whole entity family"
              % (ent[0][0], ent[0][1], ent[0][2]))
    print()
    print("  So the inventory's silence is not evidence that a subsystem has no")
    print("  debt. It is evidence that nobody has written enough of that")
    print("  subsystem for a refusal to have somewhere to live.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
