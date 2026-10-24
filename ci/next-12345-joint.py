#!/usr/bin/env python3
"""next-12345-joint.py — the ORDERED-PAIR decomposition of an adjudicated
charge, with the positional census each row needs before any class is opened.

WHY THIS IS NOT `ci/floor-census.py --top N`. That script keys its two
directions separately and marginalises each over the other side, which is
correct and is also why the same visible lump has three sizes: the MISS
marginal (keyed on the vanilla spelling), the SURPLUS marginal (keyed on what
we placed), and the ordered pair itself, which is what an item actually is.
On the 20260722 window those were 649, 648 and 646 for one lump. Ranking on a
marginal therefore over-counts a row whose two sides are not the same set of
positions, and every item this project has opened on a confusion table has
decomposed differently once the joint was taken.

WHY IT IS NOT `ci/confusion-pair-census.py` either. That adjudicates ONE named
pair, with its own floor, to close an item. You cannot name the pair until you
have the table, and this produces the table.

THE ADJUDICATED SET. A position is charged only when our block matches NONE of
the references (the admissible instrument, docs/DETERMINISM.md §12): nothing is
masked and the whole window is the denominator. Here that is the intersection
of all N `<ref>-vs-ours.pos` files. Where the references disagree with each
other about what belongs at a charged position, the vanilla side is recorded as
`<refs-split>` rather than picking one, because picking one invents a pair that
no reference states.

FLOORS ARE NOT COMPUTED HERE AND NOTHING IS SUBTRACTED. This ranks; the floor
that binds a row is the LARGER of that row's miss floor (keyed on the vanilla
spelling) and its surplus floor (keyed on what we placed), both of which come
from `ci/floor-census.py`'s two tables, and the closing adjudication comes from
`ci/confusion-pair-census.py`. A rank is not a verdict.

usage: next-12345-joint.py <posdir> --cand ours --refs a,b,c[,...] [--top N]
                           [--pair '<vanilla>-><ours>']   detail for one row
exit 0 fine, 2 malformed or missing inputs.
"""

from __future__ import annotations

import collections
import os
import sys


def die(msg: str):
    print(f"next-12345-joint: {msg}", file=sys.stderr)
    sys.exit(2)


def load(path: str) -> dict[tuple[int, int, int], tuple[str, str]]:
    out = {}
    with open(path) as fh:
        for line in fh:
            f = line.split()
            if len(f) != 5:
                continue
            try:
                x, y, z = int(f[0]), int(f[1]), int(f[2])
            except ValueError:
                continue
            out[(x, y, z)] = (f[3], f[4])
    if not out:
        die(f"{path} yielded no positions — wrong format or an empty diff")
    return out


def main(argv: list[str]) -> int:
    posdir, opts, i = None, {}, 0
    while i < len(argv):
        a = argv[i]
        if a in ("--cand", "--refs", "--top", "--pair"):
            if i + 1 >= len(argv):
                die(f"{a} needs a value")
            opts[a] = argv[i + 1]
            i += 2
            continue
        if a.startswith("--"):
            die(f"unknown option {a}")
        posdir = a
        i += 1
    if not posdir or "--cand" not in opts or "--refs" not in opts:
        die(__doc__.split("usage:")[1].strip())
    cand = opts["--cand"]
    refs = [r for r in opts["--refs"].split(",") if r]
    if len(refs) < 2:
        die("need at least two references")
    top = int(opts.get("--top", "25"))

    # ---- the N-way intersection: charged against every reference ----------
    per: dict[str, dict] = {}
    for r in refs:
        p = os.path.join(posdir, f"{r}-vs-{cand}.pos")
        if not os.path.isfile(p):
            die(f"missing {p}")
        per[r] = load(p)
    common = set(per[refs[0]])
    for r in refs[1:]:
        common &= set(per[r])

    # ---- key each position, splitting where the references disagree -------
    rows = collections.Counter()
    detail: dict[str, list] = collections.defaultdict(list)
    ours_disagree = 0
    for pos in common:
        vans = {per[r][pos][0] for r in refs}
        ours = {per[r][pos][1] for r in refs}
        if len(ours) != 1:
            # our own dump is one world; a split here means the matrix is
            # inconsistent and every count below it is void.
            ours_disagree += 1
            continue
        o = ours.pop()
        v = vans.pop() if len(vans) == 1 else "<refs-split>"
        key = f"{v} -> {o}"
        rows[key] += 1
        detail[key].append(pos)

    if ours_disagree:
        print(
            f"VOID  {ours_disagree} positions where the 16 files disagree about OUR "
            "own block — the matrix is not one dump",
            file=sys.stderr,
        )
        return 2

    total = sum(rows.values())
    print(f"JOINT CONFUSION  cand={cand}  refs={len(refs)}  charge={total:,}")
    print("  every row is the ORDERED PAIR, not either marginal")
    print(f"  {'vanilla -> ours':<62} {'count':>7}  {'share':>6}")
    for key, n in rows.most_common(top):
        print(f"  {key:<62} {n:>7,}  {100.0 * n / total:>5.1f}%")
    if len(rows) > top:
        rest = total - sum(n for _, n in rows.most_common(top))
        print(f"  {'(' + str(len(rows) - top) + ' further rows)':<62} {rest:>7,}")

    # ---- positional census, for the top rows or one named one -------------
    want = [opts["--pair"]] if "--pair" in opts else [k for k, _ in rows.most_common(3)]
    for key in want:
        if key not in detail:
            die(f"no such row: {key}")
        ps = detail[key]
        ybands = collections.Counter((p[1] // 16) * 16 for p in ps)
        chunks = collections.Counter((p[0] >> 4, p[2] >> 4) for p in ps)
        cols = {(p[0], p[2]) for p in ps}
        border = sum(1 for p in ps if p[0] % 16 in (0, 15) or p[2] % 16 in (0, 15))
        print(f"\n  POSITIONS OF  {key}   n={len(ps):,}")
        print(f"    distinct x/z columns      : {len(cols):,}  "
              f"(depth {len(ps) / len(cols):.2f} blocks per column)")
        print(f"    y range                   : {min(p[1] for p in ps)}..{max(p[1] for p in ps)}")
        print("    y bands (16)              : " + ", ".join(
            f"{b}..{b+15}:{n}" for b, n in sorted(ybands.items())))
        print(f"    chunks touched            : {len(chunks)} of 1,089")
        print("    worst chunks              : " + ", ".join(
            f"({cx},{cz}):{n}" for (cx, cz), n in chunks.most_common(5)))
        half, acc = 0, 0
        for _, n in chunks.most_common():
            acc += n
            half += 1
            if acc * 2 >= len(ps):
                break
        print(f"    half the row lives in     : {half} chunk(s)")
        print(f"    chunk-border share        : {100.0 * border / len(ps):.1f}% "
              "(12.5% is what a uniform scatter gives)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
