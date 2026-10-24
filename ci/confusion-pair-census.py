#!/usr/bin/env python3
"""Decompose ONE row of a confusion table positionally, before anyone opens a file.

# WHY THIS EXISTS

`ci/floor-census.py --adjudicate` ranks the charge by block and tells you
which rows clear their floor. It cannot tell you *where* a row is, and a row
is the thing briefs get written about. Four worldgen items this week were
named by a confusion table and decomposed differently once somebody looked
at the positions, so looking at the positions is now a script rather than an
errand.

# THE TWO MARGINALS ARE NOT THE ITEM, AND THIS IS THE TRAP

A confusion table has two directions and `floor-census.py` keys them
separately, each **marginalised over the other side**:

  * the MISS direction keys on the VANILLA spelling — "vanilla says B here
    and we do not", whatever we said instead;
  * the SURPLUS direction keys on OURS — "we placed C here and vanilla did
    not", whatever vanilla said instead.

So `B -> C` quoted off the miss table and `B -> C` quoted off the placed
table are **two different sets**, and the ordered pair is the intersection —
necessarily no larger than either. Measured on the item this script was
written for (seed 20260722, r=16, `grass_block[snowy=false] -> dirt`):

    miss     grass_block[snowy=false] -> anything   649   = 646 dirt + 3 clay
    surplus  anything -> dirt                       648   = 646 grass_block + 2 coal_ore
    the ordered pair itself                         646

Three numbers, one item. A brief that says 649 and a commit that says 646 are
not in contradiction and neither is wrong; they are the marginal and the
joint. `--pair` reports all three so the difference cannot be argued about.

# THE CHARGE

A position is charged when our block matches NONE of the references — the
rule `ci/admissible-residual.py` states at N=2, generalised to N. A position
appears in `<ref>-vs-<cand>.pos` exactly when ours differs from that ref, so
"matches none" is the INTERSECTION of the N files and needs no third pass
over the region files. Nothing is masked and nothing is subtracted.

Reads `.pos` files only. Writes nothing anywhere.

usage: confusion-pair-census.py <posdir> --cand <name> --refs a,b,c,...
                                [--pair '<vanilla>-><ours>']
                                [--vanilla <block>] [--ours <block>]
                                [--expect N] [--top N]
       confusion-pair-census.py --selftest

exit 0 fine, 1 an --expect did not match, 2 malformed or missing inputs.
"""
import os
import sys
from collections import Counter, defaultdict

OPTIONS = ("--posdir", "--cand", "--refs", "--pair", "--vanilla", "--ours", "--expect", "--top")


def die(msg, code=2):
    print(f"confusion-pair-census: {msg}", file=sys.stderr)
    sys.exit(code)


def qualify(name):
    """`dirt` and `minecraft:dirt` are the same block; a state is not a block."""
    return name if ":" in name else f"minecraft:{name}"


def load(posdir, ref, cand):
    path = os.path.join(posdir, f"{ref}-vs-{cand}.pos")
    if not os.path.exists(path):
        die(f"missing {path}")
    out = {}
    with open(path) as fh:
        for line in fh:
            p = line.split()
            if len(p) < 5:
                continue
            try:
                key = (int(p[0]), int(p[1]), int(p[2]))
            except ValueError:
                continue  # `region-differ` header/footer lines
            out[key] = (p[3], p[4])
    if not out:
        die(f"{path} holds no positions")
    return out


def census(maps, vanilla, ours):
    charged = set(maps[0])
    for m in maps[1:]:
        charged &= set(m)
    # Our spelling is a property of our dump, identical in every file. The
    # vanilla spelling is not: references may disagree with each other.
    ours_at = {p: maps[0][p][1] for p in charged}
    van_at = {p: {m[p][0] for m in maps} for p in charged}
    miss = {p for p in charged if vanilla in van_at[p]}
    surplus = {p for p in charged if ours_at[p] == ours}
    pair = miss & surplus
    return charged, miss, surplus, pair, ours_at, van_at


def report(charged, miss, surplus, pair, ours_at, van_at, vanilla, ours, top):
    print(f"charge (ours matches none of the references): {len(charged):,}")
    print()
    print("THE THREE NUMBERS")
    print(f"  MISS     {vanilla} -> anything : {len(miss):,}")
    for k, n in Counter(ours_at[p] for p in miss).most_common(top):
        mark = "  <-- the pair" if k == ours else ""
        print(f"      -> {k:<46} {n:6,}{mark}")
    print(f"  SURPLUS  anything -> {ours} : {len(surplus):,}")
    for k, n in Counter(" | ".join(sorted(van_at[p])) for p in surplus).most_common(top):
        mark = "  <-- the pair" if k == vanilla else ""
        print(f"      <- {k:<46} {n:6,}{mark}")
    print(f"  THE ORDERED PAIR (the intersection)          : {len(pair):,}")
    print("  A marginal is not the item. The joint is the item.")
    print()

    if not pair:
        return
    sel = pair
    ys = Counter(p[1] for p in sel)
    print("Y DISTRIBUTION")
    print(f"  min y {min(ys)}   max y {max(ys)}   distinct y {len(ys)}")
    bands = Counter()
    for y, n in ys.items():
        bands[(y // 16) * 16] += n
    peak = max(bands.values())
    for b in sorted(bands):
        bar = "#" * max(1, int(58 * bands[b] / peak))
        print(f"  y {b:5d}..{b + 15:<5d} {bands[b]:7,}  {bar}")
    print("  busiest single y: " + ", ".join(f"y={y}:{n:,}" for y, n in ys.most_common(5)))
    print()

    ch = Counter(((p[0] >> 4), (p[2] >> 4)) for p in sel)
    tot = sum(ch.values())
    acc = nch = 0
    for _, n in ch.most_common():
        acc += n
        nch += 1
        if acc >= tot / 2:
            break
    worst, worstn = ch.most_common(1)[0]
    print("CHUNK DISTRIBUTION")
    print(f"  chunks holding any: {len(ch):,}")
    print(f"  worst chunk {worst}: {worstn:,}")
    print(f"  half of it lives in {nch} chunk(s) ({nch / len(ch) * 100:.1f}% of the occupied ones)")
    print(f"  per-chunk mean {tot / len(ch):.1f}, median {sorted(ch.values())[len(ch) // 2]}")
    print()

    # A chunk-seam artefact piles up on local 0 and 15. Two of sixteen
    # columns is 12.5%, so that is the number to read this against.
    lx = sum(1 for p in sel if (p[0] & 15) in (0, 15))
    lz = sum(1 for p in sel if (p[2] & 15) in (0, 15))
    print("CHUNK-BORDER TEST")
    print(f"  local x on a border: {lx:,} ({lx / tot * 100:.1f}%)")
    print(f"  local z on a border: {lz:,} ({lz / tot * 100:.1f}%)")
    print("  uniform would be 12.5% each; a seam artefact reads far above it")
    print()

    cols = defaultdict(int)
    for x, y, z in sel:
        cols[(x, z)] += 1
    thick = Counter(cols.values())
    print("COLUMN THICKNESS")
    print(f"  distinct x/z columns: {len(cols):,}")
    for k in sorted(thick)[:6]:
        print(f"  {k} deep: {thick[k]:,} column(s)")
    print("  all-1 means a SKIN (a surface), not a volume")
    print()

    seen, sizes = set(), []
    for p in sel:
        if p in seen:
            continue
        stack, n = [p], 0
        seen.add(p)
        while stack:
            x, y, z = stack.pop()
            n += 1
            for d in ((1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)):
                q = (x + d[0], y + d[1], z + d[2])
                if q in sel and q not in seen:
                    seen.add(q)
                    stack.append(q)
        sizes.append(n)
    sizes.sort(reverse=True)
    print("6-CONNECTED CLUSTERS")
    print(f"  clusters {len(sizes):,}   largest {sizes[0]}   median {sizes[len(sizes) // 2]}")
    print(f"  singletons {sum(1 for s in sizes if s == 1):,}")
    print(f"  largest ten: {sizes[:10]}")
    print("  many small clusters = a rim/lip; one blob = a region-wide rule error")


def selftest():
    """A census that cannot separate the joint from its marginals is the bug
    this script exists to prevent, so that is what is injected."""
    import tempfile

    ok = True

    def check(name, got, want):
        nonlocal ok
        good = got == want
        ok &= good
        print(f"  {'ok  ' if good else 'FAIL'} {name}: got {got}, want {want}")

    with tempfile.TemporaryDirectory() as d:
        # Two refs. Positions 1..3 are the pair; 4 is a miss that is not the
        # pair; 5 is a surplus that is not the pair; 6 is charged but neither;
        # 7 is NOT charged because ref b agrees with us there.
        rows_a = [
            "0 64 0 minecraft:grass_block[snowy=false] minecraft:dirt",
            "1 64 0 minecraft:grass_block[snowy=false] minecraft:dirt",
            "2 64 0 minecraft:grass_block[snowy=false] minecraft:dirt",
            "3 64 0 minecraft:grass_block[snowy=false] minecraft:clay",
            "4 64 0 minecraft:coal_ore minecraft:dirt",
            "5 64 0 minecraft:stone minecraft:granite",
            "6 64 0 minecraft:stone minecraft:tuff",
        ]
        rows_b = rows_a[:-1]  # position 6 absent -> ref b agrees with us
        for ref, rows in (("a", rows_a), ("b", rows_b)):
            with open(os.path.join(d, f"{ref}-vs-zx.pos"), "w") as fh:
                fh.write("DIFF header line that must be skipped\n")
                fh.write("\n".join(rows) + "\n")
        maps = [load(d, r, "zx") for r in ("a", "b")]
        charged, miss, surplus, pair, _, _ = census(
            maps, "minecraft:grass_block[snowy=false]", "minecraft:dirt"
        )
        check("charge excludes the position a ref agrees on", len(charged), 6)
        check("miss marginal", len(miss), 4)
        check("surplus marginal", len(surplus), 4)
        check("the joint is smaller than both marginals", len(pair), 3)

        # The injection: if the joint were computed as either marginal, the
        # check above would read 4. Prove the script would notice.
        check("joint != miss marginal", len(pair) != len(miss), True)
        check("joint != surplus marginal", len(pair) != len(surplus), True)

        # A reference disagreeing with the others about the vanilla spelling
        # must still count toward the miss key, which is why van_at is a set.
        rows_c = ["0 64 0 minecraft:mycelium minecraft:dirt"] + rows_a[1:]
        with open(os.path.join(d, "c-vs-zx.pos"), "w") as fh:
            fh.write("\n".join(rows_c) + "\n")
        maps3 = [load(d, r, "zx") for r in ("a", "b", "c")]
        _, miss3, _, pair3, _, _ = census(
            maps3, "minecraft:grass_block[snowy=false]", "minecraft:dirt"
        )
        check("a ref that spells it differently still keys the miss", len(miss3), 4)
        check("and the joint still holds", len(pair3), 3)

    print("CONFUSION-PAIR-CENSUS " + ("GREEN" if ok else "RED"))
    sys.exit(0 if ok else 1)


def main(argv):
    if "--selftest" in argv:
        selftest()
    if not argv:
        die(__doc__.strip().split("usage:")[1])
    posdir, opts, i = None, {}, 0
    while i < len(argv):
        a = argv[i]
        if a in OPTIONS:
            if i + 1 >= len(argv):
                die(f"{a} needs a value")
            opts[a] = argv[i + 1]
            i += 2
            continue
        if a.startswith("--"):
            die(f"unknown option {a}")
        if posdir is not None:
            die("give exactly one <posdir>")
        posdir = a
        i += 1
    posdir = opts.get("--posdir", posdir)
    if not posdir:
        die("no <posdir>")
    if not os.path.isdir(posdir):
        die(f"{posdir} is not a directory")
    cand = opts.get("--cand")
    refs = opts.get("--refs")
    if not cand or not refs:
        die("--cand and --refs are both required")
    refs = [r.strip() for r in refs.split(",") if r.strip()]
    if len(refs) < 2:
        die("give at least two --refs; the charge is an intersection")

    if "--pair" in opts:
        if "->" not in opts["--pair"]:
            die("--pair looks like '<vanilla>-><ours>'")
        v, o = opts["--pair"].split("->", 1)
        vanilla, ours = qualify(v.strip()), qualify(o.strip())
    else:
        if "--vanilla" not in opts or "--ours" not in opts:
            die("give --pair, or both --vanilla and --ours")
        vanilla, ours = qualify(opts["--vanilla"]), qualify(opts["--ours"])

    top = int(opts.get("--top", "8"))
    maps = [load(posdir, r, cand) for r in refs]
    charged, miss, surplus, pair, ours_at, van_at = census(maps, vanilla, ours)
    print(f"posdir {posdir}   cand {cand}   refs {','.join(refs)} (N={len(refs)})")
    print(f"pair   {vanilla} -> {ours}")
    print()
    report(charged, miss, surplus, pair, ours_at, van_at, vanilla, ours, top)

    if "--expect" in opts:
        want = int(opts["--expect"])
        if len(pair) != want:
            print(f"\nEXPECTED the ordered pair to be {want:,}, got {len(pair):,}")
            return 1
        print(f"\nreproduces the expected ordered pair {want:,}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
