#!/usr/bin/env python3
"""Fit a displacement between TWO WHOLE BODIES of a block, not between the
difference sets a confusion table hands you.

# WHY THIS EXISTS, AND THE MISTAKE IT WAS WRITTEN TO STOP REPEATING

A miss/surplus pair of the same material — "vanilla has packed_ice here and
we do not" against "we have packed_ice here and vanilla does not" — has three
explanations a positional census cannot separate: the feature is DISPLACED,
the feature is the wrong SHAPE at the right origin, or the PREDICATE deciding
what to overwrite is wrong. Only the first is falsifiable in one command.

The obvious test is wrong and this file exists because it was written and its
own self-test caught it. Fitting a translation between the two DIFFERENCE
sets does not test displacement: for a solid body B displaced by d, the sets
are `B \\ (B+d)` and `(B+d) \\ B` — the trailing and leading faces — and the
translation carrying one onto the other is the body's WIDTH along d, not d.
For anything that is not a rectangular prism they are not translates of each
other at all, so the fit fails on a real displacement and the hypothesis gets
wrongly killed.

So this script takes the WHOLE bodies, which `region-differ census --match
<block> --positions` prints for any region directory:

    region-differ census <ref>      --bbox X1 Z1 X2 Z2 --ybox lo hi \\
        --match packed_ice --positions > van.txt
    region-differ census <ours>/region --bbox ... --match packed_ice --positions > ours.txt
    ci/feature-body-shift.py van.txt ours.txt --radius 3 --clusters

and fits the translation directly. On whole bodies the fit means what it
says: `d=(1,0,0) explains 98%` is a displacement of one in x.

# READ THE VERDICT LINES, NOT THE TOP OF THE TABLE

  * best d is the ORIGIN and both bodies are nearly equal      -> neither; look elsewhere
  * best d is the ORIGIN and one body strictly contains the other -> a SHAPE/extent error
  * best d is NONZERO and explains most of both bodies         -> a DISPLACEMENT
  * no d explains much of either                               -> a PREDICATE error, or
                                                                  two unrelated placements

`--clusters` fits each connected component separately, which is the test that
matters when a window holds several placements of one feature: a feature is
placed once per chunk, and two icebergs need not be wrong the same way. One d
across every cluster is a systematic displacement; a different d per cluster
is a per-placement input (an origin, a seed draw, a rounding), which is a
different fix.

Reads text only. Writes nothing anywhere.

usage: feature-body-shift.py <van.txt> <ours.txt> [--match <substr>]
                             [--radius N] [--clusters] [--top N]
                             [--ybox lo hi] [--expect-d dx,dy,dz]
       feature-body-shift.py --selftest

exit 0 fine, 1 an --expect-d did not match, 2 malformed or missing inputs.
"""
import os
import sys
from collections import Counter, deque


def die(msg, code=2):
    print(f"feature-body-shift: {msg}", file=sys.stderr)
    sys.exit(code)


def load(path, match=None, ybox=None):
    if not os.path.exists(path):
        die(f"missing {path}")
    out = set()
    with open(path) as fh:
        for line in fh:
            p = line.split()
            if len(p) < 4:
                continue  # `CENSUS chunks=` summary and the count table
            try:
                x, y, z = int(p[0]), int(p[1]), int(p[2])
            except ValueError:
                continue
            if match and match not in p[3]:
                continue
            if ybox and not (ybox[0] <= y <= ybox[1]):
                continue
            out.add((x, y, z))
    return out


def shifts(radius):
    for dx in range(-radius, radius + 1):
        for dy in range(-radius, radius + 1):
            for dz in range(-radius, radius + 1):
                yield (dx, dy, dz)


def fit(A, B, radius):
    """Best translation d maximising |(A+d) & B|, ranked.

    TIES GO TO THE SMALLEST DISPLACEMENT, and that is load-bearing rather than
    cosmetic. A body contained in a larger one is contained under several
    shifts at once — a radius-6 ball sits inside a radius-7 ball at the origin
    AND at every d of length 1 — so a naive `sort(reverse=True)` breaks the tie
    on the tuple and reports a displacement for a pure extent error. The
    self-test's concentric case caught exactly that. Parsimony is the right
    rule here anyway: a shift that explains no more than the origin explains
    nothing.
    """
    if not A or not B:
        return []
    out = []
    for d in shifts(radius):
        dx, dy, dz = d
        n = sum(1 for (x, y, z) in A if (x + dx, y + dy, z + dz) in B)
        out.append((n, d))
    out.sort(key=lambda t: (t[0], -(abs(t[1][0]) + abs(t[1][1]) + abs(t[1][2]))), reverse=True)
    return out


def centroid(S):
    n = len(S)
    if not n:
        return None
    return tuple(sum(p[i] for p in S) / n for i in range(3))


def clusters(S):
    """6-connected components."""
    seen, out = set(), []
    for start in S:
        if start in seen:
            continue
        comp, q = set(), deque([start])
        seen.add(start)
        while q:
            x, y, z = q.popleft()
            comp.add((x, y, z))
            for nb in ((x + 1, y, z), (x - 1, y, z), (x, y + 1, z),
                       (x, y - 1, z), (x, y, z + 1), (x, y, z - 1)):
                if nb in S and nb not in seen:
                    seen.add(nb)
                    q.append(nb)
        out.append(comp)
    out.sort(key=len, reverse=True)
    return out


def bbox(S):
    xs = [p[0] for p in S]
    ys = [p[1] for p in S]
    zs = [p[2] for p in S]
    return (min(xs), min(ys), min(zs), max(xs), max(ys), max(zs))


def verdict(A, B, ranked):
    inter = len(A & B)
    onlyA, onlyB = len(A - B), len(B - A)
    best_n, best_d = ranked[0]
    origin_n = next(n for n, d in ranked if d == (0, 0, 0))
    print("VERDICT")
    if best_d == (0, 0, 0):
        if onlyA == 0 and onlyB > 0:
            print("  SHAPE/EXTENT — same origin, ours strictly CONTAINS vanilla's body")
        elif onlyB == 0 and onlyA > 0:
            print("  SHAPE/EXTENT — same origin, ours is strictly CONTAINED BY vanilla's")
        elif onlyA or onlyB:
            print("  SHAPE/EXTENT or PREDICATE — same origin, neither body contains the other")
        else:
            print("  IDENTICAL — the bodies agree; this pair is not about this block")
    elif best_n > origin_n * 1.05 and best_n >= 0.5 * min(len(A), len(B)):
        print(f"  DISPLACEMENT d={best_d} — beats the origin ({best_n:,} vs {origin_n:,})")
    else:
        print("  NOT A DISPLACEMENT — no translation beats the origin materially")
    print(f"  |vanilla| {len(A):,}   |ours| {len(B):,}   shared {inter:,}"
          f"   vanilla-only {onlyA:,}   ours-only {onlyB:,}")
    print()


def selftest():
    ok = True

    def ball(cx, cy, cz, r):
        return {(x, y, z)
                for x in range(cx - r, cx + r + 1)
                for y in range(cy - r, cy + r + 1)
                for z in range(cz - r, cz + r + 1)
                if (x - cx) ** 2 + (y - cy) ** 2 + (z - cz) ** 2 <= r * r}

    # 1. a real displacement of the WHOLE body must fit exactly at d
    A = ball(0, 60, 0, 6)
    B = {(x + 1, y, z) for (x, y, z) in A}
    r = fit(A, B, 2)
    if r[0][1] != (1, 0, 0) or r[0][0] != len(A):
        print(f"selftest FAIL: displaced body fitted {r[0][1]} at {r[0][0]}, want (1,0,0) at {len(A)}")
        ok = False

    # 2. THE BUG THIS FILE EXISTS FOR: fitting the DIFFERENCE sets of that same
    #    displaced body must NOT recover d — proving the difference-set test is
    #    invalid, so nobody rewrites this script back into one.
    rd = fit(A - B, B - A, 2)
    if rd and rd[0][1] == (1, 0, 0) and rd[0][0] >= 0.5 * len(A - B):
        print("selftest FAIL: the difference-set fit recovered d, so the warning above is wrong")
        ok = False

    # 3. a shape error (one radius bigger, same origin) must fit the ORIGIN
    C = ball(0, 60, 0, 7)
    r3 = fit(A, C, 2)
    if r3[0][1] != (0, 0, 0):
        print(f"selftest FAIL: concentric bodies fitted {r3[0][1]}, not the origin")
        ok = False
    if A - C:
        print("selftest FAIL: the smaller ball should be contained in the larger")
        ok = False

    # 4. a one-block-taller body must fit the origin and be a strict superset
    D = A | {(x, y + 1, z) for (x, y, z) in A}
    r4 = fit(A, D, 2)
    if r4[0][1] != (0, 0, 0) or (A - D):
        print("selftest FAIL: a grown body did not read as origin-plus-superset")
        ok = False

    # 5. clusters must be found and counted independently
    E = ball(0, 60, 0, 3) | ball(40, 60, 40, 3)
    cl = clusters(E)
    if len(cl) != 2:
        print(f"selftest FAIL: two separated balls read as {len(cl)} cluster(s)")
        ok = False

    # 6. disjoint scatter must not produce a confident displacement
    import random
    random.seed(7)
    S1 = {(random.randrange(200), random.randrange(60, 70), random.randrange(200)) for _ in range(400)}
    S2 = {(random.randrange(200), random.randrange(60, 70), random.randrange(200)) for _ in range(400)}
    r6 = fit(S1, S2, 2)
    if r6[0][0] > 0.5 * len(S1):
        print("selftest FAIL: random scatter produced a confident displacement")
        ok = False

    print("FEATURE-BODY-SHIFT SELFTEST " + ("GREEN — all six cases behaved" if ok else "RED"))
    return 0 if ok else 1


def main(argv):
    if "--selftest" in argv:
        sys.exit(selftest())
    # values of options are not positional, so parse rather than filter
    opts, flags = {}, set()
    i, positional = 0, []
    while i < len(argv):
        a = argv[i]
        if a == "--clusters":
            flags.add(a)
            i += 1
        elif a == "--ybox":
            opts["--ybox"] = (int(argv[i + 1]), int(argv[i + 2]))
            i += 3
        elif a.startswith("--"):
            if i + 1 >= len(argv):
                die(f"{a} needs a value")
            opts[a] = argv[i + 1]
            i += 2
        else:
            positional.append(a)
            i += 1
    if len(positional) != 2:
        die("need exactly two position files: <vanilla> <ours>")
    match = opts.get("--match")
    ybox = opts.get("--ybox")
    radius = int(opts.get("--radius", 3))
    top = int(opts.get("--top", 10))
    A = load(positional[0], match, ybox)
    B = load(positional[1], match, ybox)
    if not A and not B:
        die("both bodies are empty — check --match and the bbox")
    print(f"vanilla {positional[0]}   ours {positional[1]}"
          + (f"   match {match}" if match else "") + f"   radius {radius}")
    print()
    ranked = fit(A, B, radius)
    verdict(A, B, ranked)
    print("TRANSLATION FIT — |(vanilla + d) & ours|")
    for n, d in ranked[:top]:
        if n == 0:
            break
        pct = 100.0 * n / len(A)
        mark = "   <-- the origin (no shift)" if d == (0, 0, 0) else ""
        print(f"  d={str(d):<14} {n:7,}  ({pct:5.1f}% of vanilla's body){mark}")
    print()
    ca, cb = centroid(A), centroid(B)
    if ca and cb:
        print("CENTROIDS")
        print(f"  vanilla ({ca[0]:.2f}, {ca[1]:.2f}, {ca[2]:.2f})")
        print(f"  ours    ({cb[0]:.2f}, {cb[1]:.2f}, {cb[2]:.2f})")
        print(f"  ours - vanilla  ({cb[0] - ca[0]:+.2f}, {cb[1] - ca[1]:+.2f}, {cb[2] - ca[2]:+.2f})")
        print("  a displacement moves the centroid; a symmetric shape error does not")
        print()
        print("BOUNDING BOXES  (x y z .. x y z)")
        print(f"  vanilla {bbox(A)}")
        print(f"  ours    {bbox(B)}")
        print()
    if "--clusters" in flags:
        print("PER-CLUSTER FIT — a feature is placed once per chunk")
        ca_list = clusters(A)
        best_ds = Counter()
        for comp in ca_list[:top]:
            if len(comp) < 8:
                continue
            r = fit(comp, B, radius)
            n, d = r[0]
            best_ds[d] += 1
            bb = bbox(comp)
            print(f"  cluster n={len(comp):5,} bbox={bb}  best d={str(d):<14}"
                  f" explains {n:5,} ({100.0 * n / len(comp):5.1f}%)")
        print(f"  distinct best-d over the clusters shown: {len(best_ds)}"
              f"   {', '.join(f'{d}:{n}' for d, n in best_ds.most_common(6))}")
        print("  one d everywhere = systematic; many = a per-placement input")
        print()
    if "--expect-d" in opts:
        want = tuple(int(v) for v in opts["--expect-d"].split(","))
        got = ranked[0][1]
        if got != want:
            print(f"EXPECT FAILED: best d={got}, expected {want}")
            sys.exit(1)
        print(f"EXPECT OK: best d={got}")


if __name__ == "__main__":
    main(sys.argv[1:])
