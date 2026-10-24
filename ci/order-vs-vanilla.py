#!/usr/bin/env python3
"""order-vs-vanilla.py — is OUR generator order-dependent where VANILLA is not?

`ci/order-stability.sh` established that 94% of the r=16 admissible charge moves
when we change our own chunk-visitation order.  That is only half a finding.
Vanilla's own r=16 floor is *also* made of chunk order — `docs/DETERMINISM.md`
§11 attributes all twenty-five of its top families to decorations that read the
world before they write it — so "our residual is order noise" and "our residual
is a defect" are both consistent with the order test on its own.

This script asks the question that separates them, per position, out of `.pos`
files that already exist:

    VV  the set of positions where any two VANILLA references disagree
    OV  the set of positions where any two of OUR chunk orders disagree

and reports the four quadrants.  The one that matters is **OV \\ VV**: positions
our order moves and vanilla's ensemble never moves.  That is literally "we are
order-dependent where vanilla is not", and if the charge concentrates there the
residual is a defect; if it concentrates in `OV & VV` it is shared noise.

It also computes the stronger, value-level form.  A position is
**order-admissible** if *some* order of ours writes a block that *some* vanilla
reference also writes there.  The N-reference admissible instrument asks whether
our one default-order answer is in vanilla's answer set; this asks whether our
*answer set* meets vanilla's at all.  A charged position that is order-admissible
is one where the only thing wrong is which order we happened to run.

WHAT THIS CANNOT DO, stated up front because the asymmetry is real and it is the
first objection anyone should raise: the eight references are eight *samples* of
vanilla's volatility, so VV is a lower bound on the set vanilla can move, while
OV is exact for the eight orders we chose (our generator is deterministic at a
fixed order — `--selftest` does not check that; `ci/order-determinism.sh` does).
So `OV \\ VV` over-counts by however much vanilla's volatile set exceeds what
eight draws reveal.  `--saturation` prints how VV grows with the number of
references, which is the evidence for or against eight being enough, and
references generated under a *different* GEN_ORDER are the direct test.

usage:
  order-vs-vanilla.py --refdir <dir> --orderdir <dir> [--refs a,b,c,...]
                      [--orders zx,zx_rev,...] [--saturation] [--extra-vv f.pos,...]

  --refdir     holds <r1>-vs-<r2>.pos for every ordered pair and <r>-vs-ours.pos
  --orderdir   holds default-vs-<order>.pos for every order
  --extra-vv   further vanilla-vs-vanilla .pos files to fold into VV (e.g. a
               reference generated under a different GEN_ORDER against a rows one)

exit 2 on a usage or input error, 0 otherwise.
"""

from __future__ import annotations

import os
import sys

# Positions are packed into one int so the big sets are sets of ints rather
# than sets of tuples: the r=16 window is 528x528x384 and the unions run to
# ~10^6 entries, where a tuple per entry costs about 5x what this does.
def _pack(x: int, y: int, z: int) -> int:
    return (((x + (1 << 20)) << 21) | (z + (1 << 20))) << 21 | (y + (1 << 20))


NAMES: dict[str, int] = {}


def nid(name: str) -> int:
    i = NAMES.get(name)
    if i is None:
        i = len(NAMES)
        NAMES[name] = i
    return i


def read_pos(path: str):
    """Yield (packed, a_id, b_id) for `x y z a_name b_name` lines."""
    with open(path, "r") as fh:
        for line in fh:
            f = line.split()
            if len(f) < 5:
                continue
            try:
                x = int(f[0]); y = int(f[1]); z = int(f[2])
            except ValueError:
                continue
            yield _pack(x, y, z), nid(f[3]), nid(f[4])


def die(msg: str) -> int:
    print(f"order-vs-vanilla: {msg}", file=sys.stderr)
    return 2


def main(argv: list[str]) -> int:
    def opt(name: str, default=None):
        if name in argv:
            i = argv.index(name)
            if i + 1 >= len(argv):
                raise SystemExit(die(f"{name} needs a value"))
            return argv[i + 1]
        return default

    refdir = opt("--refdir")
    orderdir = opt("--orderdir")
    if not refdir or not orderdir:
        return die(__doc__.split("usage:")[1].strip())
    refs = opt("--refs", "a,b,c,d,e,f,g,h").split(",")
    orders = opt("--orders", "zx,zx_rev,zx_zrev,reverse,spiral,spiral_in,spawn,diag").split(",")
    extra_vv = [p for p in (opt("--extra-vv", "") or "").split(",") if p]
    want_sat = "--saturation" in argv

    # ---- OUR order-volatile set, and our value set per position -------------
    # default-vs-<order>.pos is `x y z <default> <order>`, so field 4 also gives
    # our default answer everywhere our order moves anything.
    ours_default: dict[int, int] = {}
    ours_values: dict[int, set[int]] = {}
    per_order = {}
    OV: set[int] = set()
    for o in orders:
        p = os.path.join(orderdir, f"default-vs-{o}.pos")
        if not os.path.exists(p):
            return die(f"missing {p}")
        n = 0
        for k, dv, ov in read_pos(p):
            n += 1
            OV.add(k)
            ours_default[k] = dv
            s = ours_values.get(k)
            if s is None:
                ours_values[k] = {dv, ov}
            else:
                s.add(ov)
        per_order[o] = n

    # ---- VANILLA's volatile set --------------------------------------------
    VV: set[int] = set()
    pairs = 0
    sat_curve = []
    seen_for_sat: set[int] = set()
    for i, r1 in enumerate(refs):
        for r2 in refs:
            if r1 >= r2:
                continue
            p = os.path.join(refdir, f"{r1}-vs-{r2}.pos")
            if not os.path.exists(p):
                p = os.path.join(refdir, f"{r2}-vs-{r1}.pos")
            if not os.path.exists(p):
                continue
            pairs += 1
            for k, _a, _b in read_pos(p):
                VV.add(k)
        if want_sat:
            # VV restricted to the first i+1 references: recompute cheaply by
            # replaying only the pairs inside that prefix.
            pref = refs[: i + 1]
            seen_for_sat = set()
            for a in pref:
                for b in pref:
                    if a >= b:
                        continue
                    q = os.path.join(refdir, f"{a}-vs-{b}.pos")
                    if not os.path.exists(q):
                        q = os.path.join(refdir, f"{b}-vs-{a}.pos")
                    if not os.path.exists(q):
                        continue
                    for k, _a2, _b2 in read_pos(q):
                        seen_for_sat.add(k)
            sat_curve.append((len(pref), len(seen_for_sat)))
    vv_rows_only = len(VV)
    for p in extra_vv:
        if not os.path.exists(p):
            return die(f"missing --extra-vv {p}")
        for k, _a, _b in read_pos(p):
            VV.add(k)

    # ---- the charge: ours differs from EVERY reference ----------------------
    charge: set[int] | None = None
    van_values: dict[int, set[int]] = {}
    blocks = None
    for r in refs:
        p = os.path.join(refdir, f"{r}-vs-ours.pos")
        if not os.path.exists(p):
            return die(f"missing {p}")
        here: set[int] = set()
        for k, vv_, ov_ in read_pos(p):
            here.add(k)
            s = van_values.get(k)
            if s is None:
                van_values[k] = {vv_}
            else:
                s.add(vv_)
            ours_default.setdefault(k, ov_)
        charge = here if charge is None else (charge & here)
    assert charge is not None

    inv = {v: k for k, v in NAMES.items()}

    print(f"WINDOW  refs={len(refs)} pairs={pairs} orders={len(orders)}")
    for o in orders:
        print(f"  order {o:<10} moves {per_order[o]:>9,}")
    print()
    print(f"  OV  our orders move            {len(OV):>9,}")
    print(f"  VV  vanilla refs disagree      {len(VV):>9,}"
          + ("" if not extra_vv else f"   ({vv_rows_only:,} from --refs alone)"))
    print(f"  OV & VV                        {len(OV & VV):>9,}")
    print(f"  OV \\ VV  ours moves, vanilla's ensemble does not"
          f"   {len(OV - VV):>9,}")
    print(f"  VV \\ OV  vanilla moves, ours does not          "
          f"   {len(VV - OV):>9,}")
    print()

    if want_sat and sat_curve:
        print("  VV saturation with reference count")
        prev = 0
        for n, size in sat_curve:
            print(f"    N={n}  |VV|={size:>9,}   +{size - prev:,}")
            prev = size
        print()

    c = charge
    c_ov = c & OV
    c_stable = c - OV
    print(f"CHARGE  ours matches no reference    {len(c):>9,}")
    print(f"  order-sensitive (in OV)            {len(c_ov):>9,}"
          f"  {100.0 * len(c_ov) / max(len(c), 1):.1f}%")
    print(f"  order-stable                       {len(c_stable):>9,}"
          f"  {100.0 * len(c_stable) / max(len(c), 1):.1f}%")
    print(f"  order-sensitive AND vanilla-volatile {len(c_ov & VV):>7,}"
          f"  {100.0 * len(c_ov & VV) / max(len(c_ov), 1):.1f}% of the order-sensitive")
    print(f"  order-sensitive, vanilla STABLE      {len(c_ov - VV):>7,}"
          f"  <- we move where vanilla's ensemble does not")
    print()

    # ---- value-level: does any order of ours land inside vanilla's answers? --
    adm = 0
    non_adm_kinds: dict[tuple[int, int], int] = {}
    for k in c:
        ov_set = ours_values.get(k) or {ours_default.get(k, -1)}
        if ov_set & van_values.get(k, set()):
            adm += 1
        else:
            dv = ours_default.get(k, -1)
            vs = van_values.get(k, set())
            key = (min(vs) if vs else -1, dv)
            non_adm_kinds[key] = non_adm_kinds.get(key, 0) + 1
    print(f"ORDER-ADMISSIBLE  some order of ours writes a block some reference"
          f" also writes there")
    print(f"  order-admissible                   {adm:>9,}"
          f"  {100.0 * adm / max(len(c), 1):.1f}% of the charge")
    print(f"  NOT order-admissible               {len(c) - adm:>9,}"
          f"  <- no order of ours can produce a vanilla answer")
    print()
    top = sorted(non_adm_kinds.items(), key=lambda kv: -kv[1])[:20]
    print("  top NOT-order-admissible kinds (a vanilla answer -> our default)")
    for (v, o), n in top:
        print(f"    {inv.get(v, '?'):<34} -> {inv.get(o, '?'):<28} {n:>6,}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
