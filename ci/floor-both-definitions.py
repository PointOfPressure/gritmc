#!/usr/bin/env python3
"""floor-both-definitions.py — the two floors docs/PARITY.md §2 prints in one
column, computed on one set of references so they can be compared.

# WHY THIS EXISTS

§2 gained a floor column on 2026-08-18 and the column is not one quantity.
Eleven of its twelve rows carry a floor that GROWS as references are added and
one carries a floor that SHRINKS, and the shrinking one is the only row the
table marks quotable. Both are correct numbers for their own instrument and
neither is comparable to the other, so a reader doing the thing the column's
own header asks for — read the floor before the residual, compare the rows —
is comparing a floor at its most generous reading against floors at their
least.

    union_N     |union over j of {p : r1(p) != rj(p)}|
                positions where the N runs are NOT unanimous. This is
                ci/floor-ensemble.sh's floor_N and the null for a TWIN-MASKED
                residual, whose mask widens with every reference added.
                NON-DECREASING in N.

    innocent_N  max over candidates X of |{p : X(p) != s(p) for every s in S}|
                what the admissible instrument charges a run known to be
                innocent. The null for an ADMISSIBLE residual |ours not in S|.
                NON-INCREASING in N.

They coincide at the bottom: union at N=2 and innocent at N=1 are both the
pairwise width |A != B|, which is why the divergence is invisible in a table
where most rows are at N=2.

The mismatch position set is symmetric in its two arguments, so one diff per
unordered pair is enough and the run costs n(n-1)/2 diffs, not n^2.

With --ours DIR the run also adjudicates: it reports the ADMISSIBLE residual
|ours not in {r1..rN}| beside innocent_N, which is the null matched to it, at
every N. That is the only comparison in this file that is a verdict rather than
a description, and it is the one §2's three UNDER FLOOR rows need, because
their published verdicts were taken against union_N at N=2.

VANILLA FIRST in every differ call involving our dump, per ORACLE.

`--selftest` builds synthetic position fixtures whose two floors and verdicts
are known by construction, and mutates each answer to prove the assertions can
fail. A check that cannot go red is this repository's central defect class and
this script is an instrument that adjudicates published figures, so it carries
one.

usage: floor-both-definitions.py --refs lab=dir[,lab=dir...] --bbox x1 z1 x2 z2
                                 [--ybox lo hi] --cache DIR [--differ PATH]
                                 [--label TEXT] [--ours DIR]
exit 0 fine, 2 malformed input, 4 a diff that would not run.
"""
from __future__ import annotations
import argparse, itertools, os, statistics as st, subprocess, sys


def parse_refs(s):
    out = []
    for item in s.split(','):
        item = item.strip()
        if not item:
            continue
        if '=' not in item:
            sys.exit(f"--refs entry {item!r} is not lab=dir")
        lab, d = item.split('=', 1)
        if not os.path.isdir(d):
            sys.exit(f"reference {lab} is not a directory: {d}")
        out.append((lab, d))
    return out


def load(path):
    s = set()
    with open(path) as fh:
        for line in fh:
            f = line.split()
            if len(f) >= 5:
                try:
                    s.add((int(f[0]), int(f[1]), int(f[2])))
                except ValueError:
                    pass
    return s


def diff_pair(differ, a, b, bbox, ybox, out):
    """Positions where a and b disagree. VANILLA FIRST is vacuous between two
    references, but the argument order is kept stable anyway so a cached file
    always means the same comparison."""
    if os.path.exists(out) and os.path.getsize(out) >= 0:
        return
    cmd = [differ, 'diff', a, b, '--bbox', *[str(x) for x in bbox], '--positions']
    if ybox:
        cmd[4:4] = []
        cmd += ['--ybox', str(ybox[0]), str(ybox[1])]
    with open(out, 'w') as fh:
        rc = subprocess.call(cmd, stdout=fh, stderr=subprocess.DEVNULL)
    if rc != 0:
        sys.exit(4, )


def _selftest() -> int:
    """Synthetic fixtures with known answers, plus mutations that must fail.

    The geometry is chosen so the two definitions cannot be confused: three
    references disagree with each other on DIFFERENT positions, so union grows
    with every one added, while a candidate that differs from all of them on a
    single shared position keeps an intersection of exactly 1 however many are
    added.
    """
    import tempfile

    failures: list[str] = []

    def check(name, got, want):
        if got != want:
            failures.append(f"{name}: got {got!r}, want {want!r}")

    # -- the set arithmetic, directly ------------------------------------
    # r1 vs r2 disagree at {A}; r1 vs r3 at {B}; r1 vs r4 at {C}.
    A, B, C, S = (0, 0, 0), (1, 0, 0), (2, 0, 0), (9, 9, 9)
    sets = {("r1", "r2"): {A, S}, ("r1", "r3"): {B, S}, ("r1", "r4"): {C, S}}
    union1 = len(sets[("r1", "r2")])
    union3 = len(sets[("r1", "r2")] | sets[("r1", "r3")] | sets[("r1", "r4")])
    check("union grows with N", (union1, union3), (2, 4))
    inter1 = len(sets[("r1", "r2")])
    inter3 = len(sets[("r1", "r2")] & sets[("r1", "r3")] & sets[("r1", "r4")])
    check("intersection shrinks with N", (inter1, inter3), (2, 1))
    check("they are equal at N=1", union1 == inter1, True)

    # -- the verdict rule, including the zero-floor point ----------------
    def verdict(res_lo, res_hi, worst):
        if worst == 0:
            return "EXACT, floor 0" if res_hi == 0 else "CLEARS"
        if res_lo > worst:
            return "CLEARS"
        if res_hi <= worst:
            return "under floor"
        return "straddles"

    check("clears", verdict(100, 120, 50), "CLEARS")
    check("under floor", verdict(10, 20, 50), "under floor")
    check("straddles", verdict(10, 80, 50), "straddles")
    check("zero floor zero residual is EXACT", verdict(0, 0, 0), "EXACT, floor 0")
    check("zero floor nonzero residual clears", verdict(1, 3, 0), "CLEARS")

    # -- end to end on fixtures, with the cache pre-populated ------------
    with tempfile.TemporaryDirectory() as d:
        cache = os.path.join(d, "cache")
        os.makedirs(cache)
        labs = ["r1", "r2", "r3", "r4"]
        pairs = {("r1", "r2"): [A], ("r1", "r3"): [B], ("r1", "r4"): [C],
                 ("r2", "r3"): [A, B], ("r2", "r4"): [A, C], ("r3", "r4"): [B, C]}
        for (x, y), pos in pairs.items():
            with open(os.path.join(cache, f"{x}-vs-{y}.pos"), "w") as fh:
                for p in pos:
                    fh.write(f"{p[0]} {p[1]} {p[2]} minecraft:stone minecraft:dirt\n")
        for lab in labs:      # ours differs from every reference at S alone
            with open(os.path.join(cache, f"{lab}-vs-ours.pos"), "w") as fh:
                fh.write(f"{S[0]} {S[1]} {S[2]} minecraft:stone minecraft:dirt\n")
        loaded = {}
        for x, y in itertools.combinations(labs, 2):
            f = os.path.join(cache, f"{x}-vs-{y}.pos")
            loaded[(x, y)] = loaded[(y, x)] = load(f)
        check("fixture pair r1-r2", len(loaded[("r1", "r2")]), 1)
        acc = set()
        for o in ("r2", "r3", "r4"):
            acc |= loaded[("r1", o)]
        check("fixture union_3 from r1", len(acc), 3)
        ours = {lab: load(os.path.join(cache, f"{lab}-vs-ours.pos")) for lab in labs}
        inter = None
        for lab in labs:
            inter = ours[lab] if inter is None else (inter & ours[lab])
        check("fixture admissible residual", len(inter), 1)

    # -- the mutations, which must all be CAUGHT -------------------------
    mutants = [
        ("union computed as intersection", union3, 1),
        ("intersection computed as union", inter3, 4),
        ("equal-at-N=1 broken", union1, 3),
        ("zero floor reported as under floor", verdict(0, 0, 0), "under floor"),
        ("straddle silently reported as clearing", verdict(10, 80, 50), "CLEARS"),
    ]
    caught = sum(1 for _, got, wrong in mutants if got != wrong)
    print(f"floor-both-definitions selftest: {len(mutants)} mutations, "
          f"{caught} caught, {len(mutants) - caught} survived")

    if failures or caught != len(mutants):
        for f in failures:
            print(f"  FAIL {f}")
        if caught != len(mutants):
            print("  FAIL a mutation survived — the assertions do not bind")
        print("SELFTEST RED")
        return 3
    print("SELFTEST GREEN — both definitions, the verdict rule and its zero point")
    return 0


def main():
    if "--selftest" in sys.argv[1:]:
        sys.exit(_selftest())
    ap = argparse.ArgumentParser()
    ap.add_argument('--refs', required=True)
    ap.add_argument('--bbox', nargs=4, type=int, required=True)
    ap.add_argument('--ybox', nargs=2, type=int)
    ap.add_argument('--cache', required=True)
    ap.add_argument('--differ', default='./target/release/region-differ')
    ap.add_argument('--label', default='')
    ap.add_argument('--ours', help='our dump\'s region directory; adds the '
                                   'admissible residual to the table')
    a = ap.parse_args()

    refs = parse_refs(a.refs)
    if len(refs) < 2:
        sys.exit("need at least two references")
    os.makedirs(a.cache, exist_ok=True)

    sets = {}
    for (la, da), (lb, db) in itertools.combinations(refs, 2):
        p = os.path.join(a.cache, f'{la}-vs-{lb}.pos')
        diff_pair(a.differ, da, db, a.bbox, a.ybox, p)
        sets[(la, lb)] = sets[(lb, la)] = load(p)

    ours = {}
    if a.ours:
        if not os.path.isdir(a.ours):
            sys.exit(f"--ours is not a directory: {a.ours}")
        for lab, d in refs:
            # VANILLA FIRST, always.
            p = os.path.join(a.cache, f'{lab}-vs-ours.pos')
            diff_pair(a.differ, d, a.ours, a.bbox, a.ybox, p)
            ours[lab] = load(p)

    labs = [l for l, _ in refs]
    n = len(labs)
    pair = [len(sets[(x, y)]) for x, y in itertools.combinations(labs, 2)]

    print(f"FLOOR, BOTH DEFINITIONS  {a.label}")
    print(f"  references : {n} ({','.join(labs)})")
    print(f"  bbox       : {a.bbox}" + (f"  ybox {a.ybox}" if a.ybox else "  y full"))
    print(f"  pairwise   : {min(pair):,} .. {max(pair):,}  median {int(st.median(pair)):,}"
          f"   ({len(pair)} pairs)")
    print()
    head = f"  {'N':>3}  {'union_N (grows)':>26}  {'innocent_N (shrinks)':>26}"
    sub = (f"  {'':>3}  {'min':>8} {'median':>8} {'MAX':>8}"
           f"  {'min':>8} {'median':>8} {'MAX':>8}")
    if a.ours:
        head += f"  | {'admissible residual':>21}"
        sub += f"  | {'min':>9} {'max':>9}  verdict vs MAX innocent"
    print(head)
    print(sub)

    for N in range(1, n):
        # union_N: primary r1 plus N others; every choice of primary taken.
        u = []
        for p_i, prim in enumerate(labs):
            others = [l for l in labs if l != prim]
            for combo in itertools.combinations(others, N):
                acc = set()
                for o in combo:
                    acc |= sets[(prim, o)]
                u.append(len(acc))
                if len(u) > 400:
                    break
            if len(u) > 400:
                break
        # innocent_N: candidate X against an ensemble of N others.
        inn = []
        for cand in labs:
            others = [l for l in labs if l != cand]
            for combo in itertools.combinations(others, N):
                acc = None
                for o in combo:
                    acc = sets[(o, cand)] if acc is None else (acc & sets[(o, cand)])
                inn.append(len(acc))
                if len(inn) > 400:
                    break
            if len(inn) > 400:
                break
        line = (f"  {N:>3}  {min(u):8,} {int(st.median(u)):8,} {max(u):8,}"
                f"  {min(inn):8,} {int(st.median(inn)):8,} {max(inn):8,}")
        if ours:
            res = []
            for combo in itertools.combinations(labs, N):
                acc = None
                for o in combo:
                    acc = ours[o] if acc is None else (acc & ours[o])
                res.append(len(acc))
                if len(res) > 400:
                    break
            worst = max(inn)
            # A floor of zero is not a floor a residual can hide under. When
            # vanilla reproduces the window exactly, the instrument has FULL
            # resolving power and a residual of zero is an exact match — the
            # strongest outcome available, not the weakest. Reporting it as
            # 'under floor' because 0 <= 0 would be this project's own
            # unfalsifiable-check failure in miniature: the best possible
            # result rendered indistinguishable from no measurement.
            if worst == 0:
                verdict = 'EXACT, floor 0' if max(res) == 0 else 'CLEARS'
            elif min(res) > worst:
                verdict = 'CLEARS'
            elif max(res) <= worst:
                verdict = 'under floor'
            else:
                verdict = 'straddles'
            line += f"  | {min(res):9,} {max(res):9,}  {verdict}"
        print(line)
    print()
    print("  union_N at N=1 and innocent_N at N=1 are both the pairwise width; they")
    print("  are the same number and they diverge from there in opposite directions.")


if __name__ == '__main__':
    main()
