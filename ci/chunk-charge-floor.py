#!/usr/bin/env python3
"""chunk-charge-floor.py — the charge on ONE chunk, with its own floor beside it.

WHY THIS EXISTS. `ci/floor-census.py` adjudicates a window and `ci/
confusion-pair-census.py` adjudicates a named block pair, both over the whole
window. Neither can be asked about a chunk. The `sulfur -> cinnabar` item needs
exactly that: its 312 charged positions sit in five chunks (272 of them in
(77,48)) while the write that is believed to cause them lands in **(77,47)**,
one chunk outside the window everything was scored on. "Does vanilla put that
block there" is a question about one chunk, and answering it by widening the
window and re-reading an aggregate would bury it under 1,369 chunks of noise.

THE CONSTRUCTION is `floor-census.py`'s, restricted to a chunk and nothing
else changed:

    charge(cand) = { p : cand(p) != r(p) for EVERY r in refs }

which is the intersection of the ordered-pair position sets, so a position is
charged only when our block matches none of the references. Nothing is masked
and nothing is subtracted. The floor is the same statistic with an innocent
unmodified vanilla run substituted for our dump, leave-one-out:

    floor(x)     = { p : x(p) != r(p) for every r in refs \\ {x} },  x in refs

and the reported floor is the **max** over those readings, because a bad
reading is a property of the run rather than of the draw (rho = +0.826,
DETERMINISM §24.2) so the pool keeps meeting new bad runs as it grows.

THE KEY OF THE FLOOR TABLE IS NOT THE PAIR — §18.3, and it is the most
expensive mistake available here. Our port writing `V -> O` is adjudicated
against two floors and takes the LARGER: the **miss** floor of V (how often an
innocent run fails to spell V where the others say V) and the **surplus** floor
of O (how often an innocent run writes O where the others do not). Keyed on the
ordered pair instead, a row whose innocent counterpart substitutes some third
block scores a floor of zero and is adjudicated against nothing.

AND THE KEY IS THE FULL BLOCK STATE. `region-differ --positions` prints
canonical states, so `sulfur` and `sulfur[foo=bar]` are different keys. A
charge pooled to the bare name and looked up in a state-keyed table silently
reports a floor of zero — the failure `ci/floor-census-falsify.sh` mutates for.
This script never strips a state; `--bare` pools BOTH sides or neither, so the
charge and the floor cannot end up keyed differently from one another.

usage:
  chunk-charge-floor.py <posdir> --cand ours --refs a,b,c[,...] --chunk X Z
                        [--top N] [--bare] [--expect-charge N]
  chunk-charge-floor.py --selftest

exit 0 fine, 1 an --expect did not hold, 2 malformed or missing inputs.
"""
import os
import sys
import statistics
from collections import Counter, defaultdict

OPTIONS = ("--cand", "--refs", "--chunk", "--top", "--expect-charge")


def die(msg, code=2):
    print(f"chunk-charge-floor: {msg}", file=sys.stderr)
    sys.exit(code)


def load(path, cx, cz, bare):
    """{(x,y,z): (vanilla, ours)} for the one chunk, out of a .pos file."""
    if not os.path.exists(path):
        die(f"missing {path}")
    # `region-differ --positions` prints a DIFF/SUMMARY header, a worst-chunk
    # list and a y-band histogram around the position lines, so the file is
    # not pure data. A data line is exactly: three integers then two block
    # names. Anything else is prose and is skipped — but the file must contain
    # a terminator, because a .pos truncated mid-write would otherwise read as
    # a short and perfectly well-formed one, which is the failure the .part
    # rename in refs-12345-widen2.sh exists to stop and this is its second
    # line of defence.
    out = {}
    x0, x1 = cx * 16, cx * 16 + 15
    z0, z1 = cz * 16, cz * 16 + 15
    terminated = False
    with open(path) as fh:
        for line in fh:
            if "EXACTNESS" in line or "no comparable full chunks" in line:
                terminated = True
                continue
            f = line.split()
            if len(f) < 5:
                continue
            try:
                x, y, z = int(f[0]), int(f[1]), int(f[2])
            except ValueError:
                continue
            if not (x0 <= x <= x1 and z0 <= z <= z1):
                continue
            v, o = f[3], f[4]
            if bare:
                v, o = v.split("[")[0], o.split("[")[0]
            out[(x, y, z)] = (v, o)
    if not terminated:
        die(f"{path} has no EXACTNESS line — it is truncated, and a truncated "
            ".pos reads as a short but well-formed one")
    return out


def charge_of(posdir, cand, refs, cx, cz, bare):
    """Positions where `cand` matches NO reference in `refs`, and how it and
    the references spell them."""
    per_ref = {}
    for r in refs:
        if r == cand:
            continue
        per_ref[r] = load(os.path.join(posdir, f"{r}-vs-{cand}.pos"), cx, cz, bare)
    if not per_ref:
        die("no references to adjudicate against")
    common = set.intersection(*(set(d) for d in per_ref.values()))
    rows = {}
    for p in common:
        ours = {d[p][1] for d in per_ref.values()}
        if len(ours) != 1:
            die(f"the candidate is spelled {sorted(ours)} at {p} by different "
                "files — those .pos files were not all taken against one dump")
        van = Counter(d[p][0] for d in per_ref.values())
        agreed = van.most_common(1)[0]
        rows[p] = (agreed[0], ours.pop(), agreed[1] == len(per_ref))
    return rows


def main(argv):
    if argv[:1] == ["--selftest"]:
        return selftest()
    if not argv or argv[0].startswith("--"):
        die("usage: chunk-charge-floor.py <posdir> --cand C --refs a,b,c "
            "--chunk X Z")
    posdir, argv = argv[0], argv[1:]
    opt = {}
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--bare":
            opt["bare"] = True
            i += 1
            continue
        if a == "--chunk":
            if i + 2 >= len(argv):
                die("--chunk needs two integers")
            opt["chunk"] = (int(argv[i + 1]), int(argv[i + 2]))
            i += 3
            continue
        if a not in OPTIONS:
            die(f"unknown option {a}")
        if i + 1 >= len(argv):
            die(f"{a} needs a value")
        opt[a.lstrip("-")] = argv[i + 1]
        i += 2

    for need in ("cand", "refs"):
        if need not in opt:
            die(f"--{need} is required")
    if "chunk" not in opt:
        die("--chunk is required")
    cand = opt["cand"]
    refs = [r for r in opt["refs"].split(",") if r]
    cx, cz = opt["chunk"]
    top = int(opt.get("top", 20))
    bare = bool(opt.get("bare"))
    if len(refs) < 2:
        die("--refs needs at least two references for a leave-one-out floor")

    print(f"chunk ({cx},{cz})  blocks x {cx*16}..{cx*16+15}  z {cz*16}..{cz*16+15}")
    print(f"pool {len(refs)} references, candidate {cand!r}, "
          f"leave-one-out N={len(refs)-1}"
          + ("  [BARE NAMES: states pooled on both sides]" if bare else ""))

    # ---- our charge -------------------------------------------------------
    ours = charge_of(posdir, cand, refs, cx, cz, bare)
    print(f"\ncharge on this chunk: {len(ours)}")
    if opt.get("expect-charge") is not None and len(ours) != int(opt["expect-charge"]):
        print(f"EXPECT FAILED: charge {len(ours)} != {opt['expect-charge']}")
        return 1

    # ---- the innocent floors ---------------------------------------------
    innocent = {}
    for x in refs:
        innocent[x] = charge_of(posdir, x, [r for r in refs if r != x], cx, cz, bare)
    counts = sorted(len(v) for v in innocent.values())
    print(f"innocent floor on this chunk over {len(refs)} leave-one-out "
          f"readings: min {counts[0]}  median {statistics.median(counts):g}  "
          f"MAX {counts[-1]}")
    worst = max(innocent, key=lambda k: len(innocent[k]))
    print(f"  the max is reference {worst!r} at {len(innocent[worst])}")

    # ---- per-block floors, keyed as §18.3 requires ------------------------
    miss_floor = defaultdict(int)     # keyed on the block the OTHERS agree on
    surplus_floor = defaultdict(int)  # keyed on the block the innocent WROTE
    for x, rows in innocent.items():
        m, s = Counter(), Counter()
        for van, mine, _ in rows.values():
            m[van] += 1
            s[mine] += 1
        for k, n in m.items():
            miss_floor[k] = max(miss_floor[k], n)
        for k, n in s.items():
            surplus_floor[k] = max(surplus_floor[k], n)

    # ---- the verdict table ------------------------------------------------
    pairs = Counter((v, o) for v, o, _ in ours.values())
    disputed = sum(1 for _, _, unan in ours.values() if not unan)
    print(f"\nof the {len(ours)} charged positions, {len(ours)-disputed} have a "
          f"UNANIMOUS vanilla spelling and {disputed} do not")
    print("\n  charge  missFl  surpFl  binding  ratio  vanilla -> ours")
    for (v, o), n in pairs.most_common(top):
        mf, sf = miss_floor.get(v, 0), surplus_floor.get(o, 0)
        b = max(mf, sf)
        ratio = "inf" if b == 0 else f"{n / b:.2f}x"
        verdict = "CLEAR" if n > 2 * max(b, 1) else ("marginal" if n > b else "AT/UNDER")
        print(f"  {n:6d}  {mf:6d}  {sf:6d}  {b:7d}  {ratio:>6}  "
              f"{v} -> {o}   {verdict}")
    if len(pairs) > top:
        print(f"  ... {len(pairs)-top} further pairs")
    return 0


# --------------------------------------------------------------- selftest --
def selftest():
    """A fixture where the right answer is known and the two named failures
    are both reachable: a pair whose floor lives under the OTHER key, and a
    state that must not be pooled with its bare name."""
    import tempfile

    ok = True
    with tempfile.TemporaryDirectory() as d:
        def write(ref, cand, rows):
            with open(os.path.join(d, f"{ref}-vs-{cand}.pos"), "w") as fh:
                for (x, y, z), (v, o) in rows.items():
                    fh.write(f"{x} {y} {z} {v} {o}\n")
                fh.write("EXACTNESS 99.999999%\n")

        IN = (1232, 24, 752)      # inside chunk (77,47)
        OUT = (1216, 24, 752)     # chunk (76,47) — must be filtered out
        # ours writes cinnabar at IN where all three refs say sulfur.
        for r in ("a", "b", "c"):
            write(r, "ours", {IN: ("sulfur", "cinnabar"),
                              OUT: ("sulfur", "cinnabar")})
        # innocent a writes WATER at IN where b and c say sulfur: the miss
        # floor of `sulfur` is 1, the surplus floor of `cinnabar` is 0. Keyed
        # on the ordered pair sulfur->cinnabar the floor would read 0.
        for r in ("b", "c"):
            write(r, "a", {IN: ("sulfur", "water")})
        write("a", "b", {})
        write("c", "b", {})
        write("a", "c", {})
        write("b", "c", {})

        rc = main([d, "--cand", "ours", "--refs", "a,b,c", "--chunk", "77", "47",
                   "--expect-charge", "1"])
        if rc != 0:
            print("SELFTEST FAIL: charge on the chunk was not 1 "
                  "(the out-of-chunk position leaked in, or the "
                  "intersection is wrong)")
            ok = False
        else:
            print("selftest: chunk filter and intersection OK")

    with tempfile.TemporaryDirectory() as d:
        def write2(ref, cand, rows):
            with open(os.path.join(d, f"{ref}-vs-{cand}.pos"), "w") as fh:
                for (x, y, z), (v, o) in rows.items():
                    fh.write(f"{x} {y} {z} {v} {o}\n")
                fh.write("EXACTNESS 99.999999%\n")
        IN = (1232, 24, 752)
        for r in ("a", "b"):
            write2(r, "ours", {IN: ("sulfur[age=1]", "cinnabar")})
        write2("b", "a", {IN: ("sulfur[age=1]", "sulfur[age=2]")})
        write2("a", "b", {IN: ("sulfur[age=2]", "sulfur[age=1]")})
        print("selftest: state-keyed run (floor must attach to sulfur[age=1])")
        rc = main([d, "--cand", "ours", "--refs", "a,b", "--chunk", "77", "47"])
        ok = ok and rc == 0

    print("SELFTEST PASS" if ok else "SELFTEST FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
