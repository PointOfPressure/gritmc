#!/usr/bin/env python3
"""floor-anchor.py — the same floor, from n diffs instead of n(n-1).

# WHY THIS EXISTS

`ci/floor-vs-n.py` costed the only lever the project has left on the
seed-12345 r=16 window and found that **the vanilla boots are not the
expense**. A reference is ~60 s; the *pair matrix* is `n(n-1)` ordered
`region-differ` calls at ~2.6 s each, and it is quadratic:

```text
    n      new refs     boots        pair diffs        diffs      total
   33         17       ~17 min            816        ~35 min    ~52 min
   85         69       ~69 min          6,900        ~5.0 h     ~6.2 h
  113         97       ~1.6 h          12,416        ~9.0 h    ~10.6 h
```

That quadratic is what made "convict at a 2x margin" read as a six-to-eleven
hour job, and it is **unnecessary**. The floor does not need every pair. It
needs, at each position, the *set of blocks vanilla is known to produce there*,
and that set is recoverable from one anchor.

# THE CONSTRUCTION

Pick one reference `c` as the anchor and diff every other reference against it
— `n-1` calls. `region-differ diff c R --positions` prints, for every position
where they differ, **both** blocks: `x y z <c-block> <R-block>`. So for any
reference R and any position p,

    val_R(p) = R's block at p = the R-column of c-vs-R at p, or c(p) if absent

because a position missing from the diff is one where R agrees with the anchor.
That is total: every reference's spelling at every position in the window is
recoverable from its own anchor diff plus the anchor. Hence the admissible set

    V_S(p) = { val_R(p) : R in S }

and the charge of any candidate X, itself needing only `c-vs-X`,

    floor(X, S) = | { p : val_X(p) not in V_S(p) } |

**Both branches matter and the second is the one that is easy to miss.** A
position where X agrees with the anchor is normally safe — but if *every*
reference in S differs from the anchor there, and none of them happens to
spell it the anchor's way, then X's (= the anchor's) block is not in V_S and X
is charged. Dropping that branch would silently under-count, so it is computed
explicitly and the self-check below is what proves it was.

Cost falls from `n(n-1)` diffs to `n`. At n=85 that is 84 diffs of about four
minutes total, against 6,900 diffs of about five hours.

# WHY THIS IS SAFE TO BELIEVE

Because it is not argued, it is **checked against the instrument it replaces**.
`--verify-against <posdir>` recomputes every candidate's floor by the direct
`n(n-1)`-matrix intersection and refuses unless the two agree **exactly, for
every candidate**. On the sixteen `rows` references of the seed-12345 r=16
overworld window the direct method reads 141 / 590 / 2,241 and this one must
reproduce all sixteen per-candidate values to the block, not merely the summary
statistics — a min, a median and a max can all three be right while the
underlying assignment is wrong.

**One caveat, stated because it is a real limit and not a formality.** The
anchor is a reference, so it cannot also be a candidate: `floor(c, S)` needs a
different anchor. With two anchors every candidate is covered at a cost of
`2n-3` diffs, which is still linear. `--anchor` and `--anchor2` name them, and
the script refuses to score a candidate against its own anchor rather than
quietly reporting a number that omits it.

# THE REGISTERED RESULT THIS SCRIPT PRODUCES — `ow-r16-12345-level-n39-iceberg`

Reproduce with `ci/refs-12345-widen2.sh` (the forty references), then
`ci/anchors-12345.sh` (the 2n-3 diffs), then:

    python3 ci/floor-anchor.py --pos ~/rp-logs-floor12345-w2/pos \
        --refs c,d,...,ap --anchor c --anchor2 d --ns 40 --cand ours

      n   N       min   median      MAX
     40  39         0        1      362

    CHARGE ours            :     1,279 at N=40

and the control that must come out of the same directory, restricted to the
original sixteen references — 141 / 590 / 2,241 with a charge of 1,329, which
is `ow-r16-12345-floor-n15` and `ow-r16-12345-level-n15-iceberg` unchanged.

usage:
  floor-anchor.py --pos DIR --refs a,b,... [--anchor c] [--anchor2 d]
                  [--cand ours] [--ns 2,4,...] [--pools 40] [--label TEXT]
                  [--verify-against DIR] [--selfcheck-floor]
  floor-anchor.py --selftest
exit 0 fine, 2 malformed input, 3 a self-check or a verification that failed.
"""

from __future__ import annotations

import importlib.util
import os
import math
import random
import statistics
import sys
from itertools import combinations

_HERE = os.path.dirname(os.path.abspath(__file__))
_PARSER = None


def _load_parser():
    global _PARSER
    if _PARSER is not None:
        return _PARSER
    path = os.path.join(_HERE, "admissible-residual.py")
    spec = importlib.util.spec_from_file_location("admissible_residual", path)
    if spec is None or spec.loader is None:  # pragma: no cover - defensive
        raise RuntimeError(f"cannot load {path}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    _PARSER = mod
    return mod


class Refuse(Exception):
    """A self-check or a verification that did not reproduce. Exit 3."""


def subsets(refs: list[str], n: int, want: int, rng: random.Random):
    """Up to `want` distinct n-subsets of `refs`, WITHOUT enumerating them all.

    **This function exists because the obvious spelling OOM-killed a real run.**
    `list(combinations(refs, n))` is fine at sixteen references — C(16,8) is
    12,870 — and at forty it is C(40,20) = 137,846,528,820, which the kernel
    ends rather than allocates. The failure is loud, which is the lucky half;
    the unlucky half is that it only appears once the pool is big enough to be
    interesting, i.e. exactly when the answer starts to matter. Anyone taking
    this to n=85 would have hit it again.

    Below the threshold the full enumeration is kept, because exhausting the
    subsets is a stronger statement than sampling them and the rungs where it
    is affordable are the rungs a reader checks by hand.
    """
    total = math.comb(len(refs), n)
    if total <= want:
        return list(combinations(refs, n))
    seen: set[tuple[str, ...]] = set()
    out = []
    # Sampling with replacement then de-duplicating: at these sizes a collision
    # is vanishingly unlikely, and the guard costs nothing and cannot loop
    # forever because `total > want` is already established.
    guard = 0
    while len(out) < want and guard < want * 20:
        guard += 1
        pick = tuple(sorted(rng.sample(refs, n)))
        if pick in seen:
            continue
        seen.add(pick)
        out.append(pick)
    return out


def load_anchor(posdir: str, anchor: str, others: list[str]):
    """(anchor_block_at, {ref: {pos: block}}, blocks) from n-1 anchor diffs.

    `anchor_block_at` is the union of every anchor spelling seen in any of the
    diffs. A position absent from it was never in any diff, so every reference
    agrees with the anchor there and it can never be charged — which is why the
    window's other 107 million positions cost nothing to carry.
    """
    ar = _load_parser()
    anchor_at: dict[tuple[int, int, int], str] = {}
    per_ref: dict[str, dict[tuple[int, int, int], str]] = {}
    blocks = -1
    interned: dict[str, str] = {}
    for r in others:
        path = os.path.join(posdir, f"{anchor}-vs-{r}.pos")
        flip = False
        if not os.path.exists(path):
            path = os.path.join(posdir, f"{r}-vs-{anchor}.pos")
            flip = True
            if not os.path.exists(path):
                raise ar.Malformed(
                    f"no anchor diff for {r}: neither {anchor}-vs-{r}.pos nor "
                    f"{r}-vs-{anchor}.pos exists in {posdir}"
                )
        pos, b, _ = ar.load(path)
        if blocks < 0:
            blocks = b
        elif b != blocks:
            raise ar.Malformed(
                f"{path} covers {b:,} blocks against {blocks:,}. A floor has "
                "to be built over one window."
            )
        d: dict[tuple[int, int, int], str] = {}
        for p, (first, second) in pos.items():
            # The FIRST column is the first argument to `region-differ diff`.
            a_blk, r_blk = (second, first) if flip else (first, second)
            a_blk = interned.setdefault(a_blk, a_blk)
            r_blk = interned.setdefault(r_blk, r_blk)
            prev = anchor_at.get(p)
            if prev is None:
                anchor_at[p] = a_blk
            elif prev != a_blk:
                raise ar.Malformed(
                    f"{path}: the anchor spells {p} as {a_blk} here and as "
                    f"{prev} in another diff. Two different anchors, or two "
                    "different windows, have been mixed."
                )
            d[p] = r_blk
        per_ref[r] = d
    # THE ANCHOR IS A REFERENCE TOO, and it belongs in the table as an EMPTY
    # diff rather than as a special case at every lookup: it differs from
    # itself nowhere, so `per_ref[anchor].get(p)` is always None and the
    # `or anchor` fallback yields its own block, which is correct. It also
    # makes branch 2 fall out right — if the anchor is in the reference set
    # then no position has every reference differing from the anchor, the
    # intersection is empty, and branch 2 contributes nothing. Without this
    # the first candidate scored against a set containing the anchor raised
    # KeyError, which is the lucky failure; the unlucky one would have been a
    # silently smaller reference set.
    per_ref.setdefault(anchor, {})
    return anchor_at, per_ref, blocks


def floor_anchor(anchor_at, per_ref, cand_d, refset: list[str]) -> int:
    """|X's block is not among the blocks S is known to produce|.

    Only positions that appear in SOME anchor diff can ever be charged, and the
    two branches are kept separate on purpose — the second is the one an
    obvious implementation drops.
    """
    charged = 0
    # Branch 1: positions where the candidate differs from the anchor.
    for p, x_blk in cand_d.items():
        a_blk = anchor_at.get(p)
        hit = False
        for r in refset:
            v = per_ref[r].get(p)
            if (a_blk if v is None else v) == x_blk:
                hit = True
                break
        if not hit:
            charged += 1
    # Branch 2: positions where the candidate AGREES with the anchor, but no
    # reference in S spells it the anchor's way. The candidate's block is then
    # the anchor's and is still not in V_S.
    if refset:
        acc = None
        for r in refset:
            keys = per_ref[r].keys()
            acc = set(keys) if acc is None else (acc & keys)
            if not acc:
                break
        for p in acc or ():
            if p in cand_d:
                continue  # already considered in branch 1
            a_blk = anchor_at.get(p)
            if all(per_ref[r].get(p) != a_blk for r in refset):
                charged += 1
    return charged


def direct_floor(posdir: str, cand: str, refset: list[str]) -> int:
    """The instrument this replaces: intersect the n-1 pairwise position sets."""
    ar = _load_parser()
    acc = None
    for r in refset:
        path = os.path.join(posdir, f"{r}-vs-{cand}.pos")
        if not os.path.exists(path):
            path = os.path.join(posdir, f"{cand}-vs-{r}.pos")
        pos, _, _ = ar.load(path)
        s = set(pos)
        acc = s if acc is None else (acc & s)
        if not acc:
            break
    return len(acc or ())


def selftest() -> int:
    import tempfile
    from pathlib import Path

    ok = True

    def expect(lbl, got, want):
        nonlocal ok
        if got != want:
            print(f"SELFTEST FAIL [{lbl}]: {got!r}, expected {want!r}")
            ok = False
        else:
            print(f"SELFTEST ok   [{lbl}]: {got!r}")

    S = "DIFF chunks=1 (a=1 b=1)  blocks=100 mismatches={n}  biomes=4/0\n"

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)

        def write(name, lines):
            (d / name).write_text("".join(lines) + S.format(n=len(lines)),
                                  encoding="utf-8")

        # Anchor c. Four runs c, x, y, z, spelled explicitly:
        #   p1: c=stone  x=dirt   y=dirt   z=dirt    -> x's dirt IS produced by y
        #   p2: c=stone  x=dirt   y=stone  z=stone   -> x alone differs
        #   p3: c=stone  x=stone  y=dirt   z=gravel  -> BRANCH 2: every ref
        #        differs from the anchor and neither spells it stone, so x's
        #        stone is not in V and x IS charged.
        L = "{x} {y} {z} {a} {b}\n"
        write("c-vs-x.pos", [L.format(x=1, y=1, z=1, a="stone", b="dirt"),
                             L.format(x=2, y=2, z=2, a="stone", b="dirt")])
        write("c-vs-y.pos", [L.format(x=1, y=1, z=1, a="stone", b="dirt"),
                             L.format(x=3, y=3, z=3, a="stone", b="dirt")])
        write("c-vs-z.pos", [L.format(x=1, y=1, z=1, a="stone", b="dirt"),
                             L.format(x=3, y=3, z=3, a="stone", b="gravel")])

        anchor_at, per_ref, blocks = load_anchor(str(d), "c", ["x", "y", "z"])
        expect("blocks", blocks, 100)
        # x against {y,z}: p1 covered by y (dirt); p2 nobody produces dirt ->
        # charged; p3 branch 2 -> charged. Total 2.
        expect("floor(x,{y,z})", floor_anchor(
            anchor_at, per_ref, per_ref["x"], ["y", "z"]), 2)
        # y against {x,z}: p1 dirt produced by x -> safe. p3 y=dirt, z=gravel,
        # x=stone(anchor) -> dirt not produced by x or z -> charged. Total 1.
        expect("floor(y,{x,z})", floor_anchor(
            anchor_at, per_ref, per_ref["y"], ["x", "z"]), 1)
        # z against {x,y}: p1 safe, p3 gravel unmatched -> 1.
        expect("floor(z,{x,y})", floor_anchor(
            anchor_at, per_ref, per_ref["z"], ["x", "y"]), 1)
        # BRANCH 2 IS LOAD-BEARING: drop it and floor(x,{y,z}) reads 1.
        only1 = sum(
            1 for p, xb in per_ref["x"].items()
            if all((per_ref[r].get(p) or anchor_at.get(p)) != xb
                   for r in ["y", "z"])
        )
        expect("branch 2 is load-bearing", (only1, 2), (1, 2))

        # N=1 must be >= N=2 (a floor can only shrink as references are added).
        expect("monotone", floor_anchor(
            anchor_at, per_ref, per_ref["x"], ["y"]) >= 2, True)

        # A mixed anchor is a refusal, not a wrong number.
        write("c-vs-w.pos", [L.format(x=1, y=1, z=1, a="granite", b="dirt")])
        try:
            load_anchor(str(d), "c", ["x", "y", "z", "w"])
            expect("mixed anchor refused", "no raise", "Malformed")
        except Exception as e:  # noqa: BLE001
            expect("mixed anchor refused", type(e).__name__, "Malformed")

    print("SELFTEST", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main(argv: list[str]) -> int:
    if "--selftest" in argv:
        return selftest()

    def opt(name, default=None):
        return argv[argv.index(name) + 1] if name in argv else default

    posdir = opt("--pos")
    if not posdir or not os.path.isdir(posdir):
        print(f"floor-anchor.py: --pos DIR required (got {posdir!r})",
              file=sys.stderr)
        return 2
    refs = (opt("--refs") or "c,d,e,f,g,h,i,j,k,l,m,n,o,p,q,r").split(",")
    anchor = opt("--anchor", refs[0])
    anchor2 = opt("--anchor2", refs[1])
    label = opt("--label", os.path.basename(posdir.rstrip("/")))
    ns = [int(x) for x in (opt("--ns") or "16").split(",")]
    pools = int(opt("--pools", "40"))
    rng = random.Random(20260810)

    if anchor not in refs or anchor2 not in refs or anchor == anchor2:
        print("floor-anchor.py: --anchor and --anchor2 must be two distinct "
              "members of --refs", file=sys.stderr)
        return 2

    a1_at, a1_ref, blocks = load_anchor(
        posdir, anchor, [r for r in refs if r != anchor])
    a2_at, a2_ref, _ = load_anchor(
        posdir, anchor2, [r for r in refs if r != anchor2])

    def floor_any(x: str, refset: list[str]) -> int:
        """Score x against refset, on whichever anchor is not x itself.

        The anchor cannot be its own candidate: its diff against itself is
        empty by construction, so branch 1 would find nothing and the answer
        would be silently zero rather than wrong-looking. Hence two anchors.
        """
        if x != anchor:
            return floor_anchor(a1_at, a1_ref, a1_ref[x], refset)
        return floor_anchor(a2_at, a2_ref, a2_ref[x], refset)

    print(f"FLOOR-ANCHOR {label}")
    print(f"  references            : {len(refs)}, anchors {anchor} / {anchor2}")
    print(f"  window                : {blocks:,} blocks")
    print(f"  diffs consumed        : {2 * len(refs) - 3} "
          f"(the pair matrix would be {len(refs) * (len(refs) - 1)})")

    # -------------------------------------------------------- verification --
    if opt("--verify-against"):
        vdir = opt("--verify-against")
        bad = []
        for x in refs:
            others = [r for r in refs if r != x]
            got = floor_any(x, others)
            want = direct_floor(vdir, x, others)
            if got != want:
                bad.append((x, got, want))
        if bad:
            raise Refuse(
                "the anchor construction disagrees with the pair matrix for "
                + ", ".join(f"{x}: {g} vs {w}" for x, g, w in bad)
            )
        print(f"  VERIFIED              : all {len(refs)} candidates agree with "
              f"the n(n-1) matrix in {os.path.basename(vdir.rstrip('/'))}")

    print()
    print(f"    {'n':>3} {'N':>3}  {'min':>8} {'median':>8} {'MAX':>8}")
    last = None
    for n in ns:
        if n < 2 or n > len(refs):
            continue
        picks = subsets(refs, n, pools, rng)
        mins, meds, maxs = [], [], []
        for pool in picks:
            vals = [floor_any(x, [r for r in pool if r != x]) for x in pool]
            mins.append(min(vals))
            meds.append(int(statistics.median(vals)))
            maxs.append(max(vals))
        last = (int(statistics.median(mins)), int(statistics.median(meds)),
                int(statistics.median(maxs)))
        print(f"    {n:>3} {n - 1:>3}  {last[0]:>8,} {last[1]:>8,} "
              f"{last[2]:>8,}")

    # ------------------------------------------------------------- charge --
    # The candidate needs ONE diff: `<anchor>-vs-<cand>.pos`. Our dump never
    # has to be diffed against the new references at all, because every
    # reference's spelling is already recoverable from its own anchor diff.
    # So widening the ensemble costs 2 diffs per reference and ZERO per
    # candidate, which is the second half of why §24.7 changes the verdict.
    cand = opt("--cand")
    if cand:
        cdir = opt("--cand-pos", posdir)
        ar = _load_parser()
        cpath = os.path.join(cdir, f"{anchor}-vs-{cand}.pos")
        if not os.path.exists(cpath):
            raise Refuse(
                f"no {os.path.basename(cpath)} in {cdir}. The candidate is "
                f"scored on the anchor's diff against it, so the anchor must "
                f"be {anchor} on both sides."
            )
        cpos, cb, _ = ar.load(cpath)
        if cb != blocks:
            raise Refuse(
                f"{cpath} covers {cb:,} blocks against the references' "
                f"{blocks:,}. Not the same window."
            )
        cand_d = {p: v[1] for p, v in cpos.items()}
        for p, (a_blk, _x) in cpos.items():
            prev = a1_at.get(p)
            if prev is not None and prev != a_blk:
                raise Refuse(
                    f"{cpath} spells the anchor at {p} as {a_blk}, the "
                    f"reference diffs spell it {prev}. The candidate was "
                    "diffed against a different anchor run."
                )
            a1_at.setdefault(p, a_blk)
        ch = floor_anchor(a1_at, a1_ref, cand_d, [r for r in refs if r != anchor])
        ch_all = floor_anchor(a1_at, a1_ref, cand_d, refs)
        print()
        print(f"  CHARGE {cand:<16}: {ch_all:>9,} at N={len(refs)}  "
              f"({ch:,} at N={len(refs) - 1}, anchor excluded)")
        if opt("--expect-charge"):
            want = int(opt("--expect-charge"))
            if ch_all != want:
                raise Refuse(
                    f"charge reads {ch_all:,}, expected {want:,}"
                )
            print(f"  charge check OK       : {ch_all:,} == expected {want:,}")

    if "--selfcheck-floor" in argv:
        want = (141, 590, 2241)
        if last != want:
            raise Refuse(
                f"pool {ns[-1]} reads {last}, the registered "
                f"ow-r16-12345-floor-n15 is {want}"
            )
        print(f"\n  selfcheck-floor OK    : {last} == registered {want}")

    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Refuse as exc:
        print(f"REFUSED: {exc}", file=sys.stderr)
        sys.exit(3)
    except Exception as exc:  # noqa: BLE001
        print(f"floor-anchor.py: {exc}", file=sys.stderr)
        sys.exit(2)
