#!/usr/bin/env python3
"""floor-vs-n.py — what does a reference COST, and what does it BUY?

# WHY THIS EXISTS

`docs/DETERMINISM.md` §18.8 has exactly two rungs of the seed-12345 r=16
overworld floor: pool 6 / N=5 gives 2,577..4,835 median 3,387, and pool 16 /
N=15 gives 141..2,241 median 590. Two points determine any two-parameter fit
you care to name, so the ladder as it stands cannot distinguish a power law
from an exponential from a floor that has already bottomed out — and the whole
question now in front of the project is an **extrapolation**: the window's
charge is 1,329 (`ow-r16-12345-level-n15-iceberg`) against a max floor of
2,241, so it sits at 0.59x and no percentage may be quoted from it. The only
lever is more references. Nobody had costed that.

This script measures the intervening rungs. It needs **no new references and
no new diffs**: every rung is a set operation over the `<ref>-vs-<cand>.pos`
matrix that `ci/next-12345-measure.sh` already writes, and the sixteen `rows`
references already exist.

# THE QUANTITY, AND WHY IT IS THE MAX OVER THE POOL

For a candidate X and a reference set S not containing X,

    floor(X, S) = | positions where X differs from EVERY reference in S |

is what the admissible instrument charges a run known to be innocent. The
number a figure is quoted against is the **worst** such reading over the
candidates available, because a defect smaller than that is indistinguishable
from having drawn an unlucky vanilla run.

§18.8 found the trap in this and it is the reason this script reports the curve
it does. **A max is an extreme-value statistic over the pool, so a larger pool
gets more chances to draw a bad candidate**: on the identical six candidates
the floor's worst reading fell 4,835 -> 1,210 going from N=5 to N=15, while the
max over all sixteen read 2,241. Both are true and only one of them is the
number the window will be quoted against.

So two curves are emitted and they answer different questions:

* **POOL** — pool of size n, leave-one-out (N = n-1), min/median/max over the
  n candidates in that pool. This is *exactly the quantity a figure at pool n
  would carry*, extreme-value inflation included, and it is the one to
  extrapolate for "how many references do I have to buy".
* **FIXED** — the same candidates every time (the first six, c..h), floor read
  at each N. This isolates what a reference is *worth* from how many chances
  a bigger pool gets to look bad, and it is the honest way to say whether the
  instrument is improving.

Both are sampled over many random pools/subsets rather than one, because a
floor belongs to the reference set that produced it (AGENT-BRIEF) and a single
draw is a sample of size one of that variation.

# THE SELF-CHECKS, WHICH ARE REFUSALS

Two registered figures must fall out of this arithmetic or the run aborts:

* `--selfcheck-floor` : pool 16, N=15 must read min 141 / median 590 / max
  2,241 over the sixteen leave-one-out candidates.
* `--selfcheck-charge`: the 16-way intersection of `<ref>-vs-ours.pos` must be
  1,329.

They are checks in opposite directions. The floor reads no code of ours at all
and must reproduce on any tree, so it can be a hard refusal. The charge reads
our dump, so it is only a valid check when pointed at the `.pos` directory of
the tip it was taken on — which is why it is opt-in and names its figure.

# THE Y-BAND, AND WHY IT IS HERE RATHER THAN IN A SECOND SCRIPT

§18.2 censused the floor and found **15,893 of 21,650 floor positions in
y 0..47**, made 55.5% of diorite/andesite/granite substituting for each other.
That is a statement about *where the instrument is blind*, and it invites the
obvious move: quote the window above the blindness. `--ybox lo,hi` restricts
every set — floor and charge alike — to a y band, at zero cost in references,
so the alternative to buying references can be costed in the same units as
buying them.

**It is offered with its own disqualifier attached.** §18.7 killed the
chunk-selected sub-window as a mirage: choosing the region by where the floor
reads zero is selection on the estimate, and the held-out test showed
790..2,239 blocks of floor leaking into regions that read zero on the
estimating half. A y band chosen by looking at this same census is the same
move in a different coordinate. So `--holdout` runs §18.7's test on whatever
band is asked for: estimate on half the candidates, read the other half's
charge inside the band, and report the leak. A band that cannot survive that
is a mirage and the script says so rather than printing an encouraging number.

usage:
  floor-vs-n.py --pos DIR [--refs c,d,...] [--ns 2,4,...] [--pools N]
                [--ybox lo,hi] [--charge-pos DIR] [--holdout K]
                [--selfcheck-floor] [--selfcheck-charge N] [--label TEXT]
  floor-vs-n.py --selftest
exit 0 fine, 2 malformed input, 3 a self-check that did not reproduce.
"""

from __future__ import annotations

import importlib.util
import os
import random
import statistics
import sys
from itertools import combinations

_HERE = os.path.dirname(os.path.abspath(__file__))

_PARSER = None


def _load_parser():
    """Borrow `load()` from admissible-residual.py — hyphen, so no plain import.

    Cached for the reason `ci/admissible-floor.py` documents: loading the
    module twice makes two distinct `Malformed` classes and the `except` in
    `main` would then not catch what the loader raised.
    """
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
    """A self-check that did not reproduce. Exit 3, never a printed warning."""


def pack(p: tuple[int, int, int]) -> int:
    """(x, y, z) -> one int, so a 4M-position matrix fits in memory as sets."""
    x, y, z = p
    return ((x + 2097152) << 42) | ((z + 1048576) << 21) | (y + 1024)


def load_matrix(
    posdir: str,
    refs: list[str],
    cands: list[str],
    ybox: tuple[int, int] | None,
) -> tuple[dict[tuple[str, str], set[int]], int]:
    """`{(ref, cand): positions}` for every ordered pair with ref != cand.

    The y filter is applied at load, not later: the sets are the only thing
    that is large and filtering them afterwards would double the peak.
    """
    ar = _load_parser()
    mat: dict[tuple[str, str], set[int]] = {}
    blocks = -1
    for cand in cands:
        for ref in refs:
            if ref == cand:
                continue
            path = os.path.join(posdir, f"{ref}-vs-{cand}.pos")
            if not os.path.exists(path):
                # The position set is symmetric; the ORDERED pair only fixes
                # which block is printed first, and nothing here reads blocks.
                alt = os.path.join(posdir, f"{cand}-vs-{ref}.pos")
                if not os.path.exists(alt):
                    raise ar.Malformed(
                        f"no diff for the pair ({ref}, {cand}): neither "
                        f"{os.path.basename(path)} nor {os.path.basename(alt)} "
                        "exists. An ensemble missing a pair is not an ensemble "
                        "of that size."
                    )
                path = alt
            pos, b, _ = ar.load(path)
            if blocks < 0:
                blocks = b
            elif b != blocks:
                raise ar.Malformed(
                    f"{path} covers {b:,} blocks against {blocks:,}. A floor "
                    "has to be built over one window."
                )
            if ybox is None:
                mat[(ref, cand)] = {pack(p) for p in pos}
            else:
                lo, hi = ybox
                mat[(ref, cand)] = {pack(p) for p in pos if lo <= p[1] <= hi}
    return mat, blocks


def floor_of(mat: dict[tuple[str, str], set[int]], cand: str, refset) -> int:
    """|X differs from every reference in refset| — smallest set intersected first.

    Ordering matters for speed, not for the answer: the intersection collapses
    from ~17k to a few hundred within two or three references, so sorting by
    size turns fifteen full passes into two.
    """
    sets = sorted((mat[(r, cand)] for r in refset), key=len)
    acc = sets[0]
    for s in sets[1:]:
        acc = acc & s
        if not acc:
            break
    return len(acc)


def pool_curve(
    mat, cands: list[str], ns: list[int], pools: int, rng: random.Random
) -> list[tuple[int, int, list[int], list[int], list[int]]]:
    """For each pool size n: the (min, median, max) a figure at that n carries.

    Returns (n, N, mins, medians, maxes) with one entry per sampled pool, so
    the caller can report the spread of the statistic and not just the
    statistic of one draw.
    """
    out = []
    for n in ns:
        if n < 2 or n > len(cands):
            continue
        allsub = list(combinations(cands, n))
        picks = allsub if len(allsub) <= pools else rng.sample(allsub, pools)
        mins, meds, maxs = [], [], []
        for pool in picks:
            vals = [floor_of(mat, x, [r for r in pool if r != x]) for x in pool]
            mins.append(min(vals))
            meds.append(int(statistics.median(vals)))
            maxs.append(max(vals))
        out.append((n, n - 1, mins, meds, maxs))
    return out


def fixed_curve(
    mat, cands: list[str], fixed: list[str], ns: list[int], pools: int,
    rng: random.Random,
) -> list[tuple[int, list[int], list[int]]]:
    """The SAME candidates at every N — §18.8's like-for-like control.

    Returns (N, maxes, medians) over sampled reference subsets of size N drawn
    from the candidates NOT under test, so no candidate is ever its own
    reference.
    """
    out = []
    for N in ns:
        maxs, meds = [], []
        for _ in range(pools):
            vals = []
            for x in fixed:
                avail = [r for r in cands if r != x]
                if N > len(avail):
                    break
                vals.append(floor_of(mat, x, rng.sample(avail, N)))
            if len(vals) == len(fixed):
                maxs.append(max(vals))
                meds.append(int(statistics.median(vals)))
        if maxs:
            out.append((N, maxs, meds))
    return out


def holdout(
    mat, cands: list[str], k: int, splits: int, rng: random.Random
) -> tuple[list[int], list[int]]:
    """§18.7's test, applied to a y band: estimate on half, read the other half.

    Returns (est_maxes, held_maxes). If the held-out half is charged materially
    more than the estimating half predicted, the band was selection on the
    estimate and not a property of the window.
    """
    est_m, held_m = [], []
    for _ in range(splits):
        shuf = cands[:]
        rng.shuffle(shuf)
        a, b = shuf[:k], shuf[k:]
        if len(a) < 2 or len(b) < 1:
            continue
        est_m.append(max(floor_of(mat, x, [r for r in a if r != x]) for x in a))
        held_m.append(max(floor_of(mat, x, a) for x in b))
    return est_m, held_m


def charge_of(posdir: str, refs: list[str], cand: str, ybox) -> int:
    """The N-way intersection of `<ref>-vs-<cand>.pos` — the admissible charge."""
    ar = _load_parser()
    acc = None
    for r in refs:
        pos, _, _ = ar.load(os.path.join(posdir, f"{r}-vs-{cand}.pos"))
        if ybox is None:
            s = {pack(p) for p in pos}
        else:
            lo, hi = ybox
            s = {pack(p) for p in pos if lo <= p[1] <= hi}
        acc = s if acc is None else (acc & s)
    return 0 if acc is None else len(acc)


def _span(v: list[int]) -> str:
    return f"{min(v):>6,}..{max(v):<6,} med {int(statistics.median(v)):>6,}"


def selftest() -> int:
    """Prove this script can fail, on fixtures, with no box, no jar, no build.

    Every case is one way a real run can be wrong. A checker whose refusals are
    never exercised is this project's most-found defect class.
    """
    import tempfile
    from pathlib import Path

    ok = True

    def expect(lbl: str, got, want) -> None:
        nonlocal ok
        if got != want:
            print(f"SELFTEST FAIL [{lbl}]: {got!r}, expected {want!r}")
            ok = False
        else:
            print(f"SELFTEST ok   [{lbl}]: {got!r}")

    S = "DIFF chunks=1 (a=1 b=1)  blocks=100 mismatches={n}  biomes=4/0\n"

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)

        def write(name: str, lines: list[str]) -> None:
            (d / name).write_text(
                "".join(lines) + S.format(n=len(lines)), encoding="utf-8"
            )

        # Three runs a,b,c, with a CONSISTENT difference relation — an
        # arbitrary set of pairwise sets is not one, and the first version of
        # this fixture was not, which is how it asserted an answer of 1 where
        # the honest answer is 0. Written out as the underlying spellings:
        #   (1,10,1)  a differs, b == c        -> floor a:yes  b:no   c:no
        #   (2,10,2)  b differs, a == c        -> floor a:no   b:yes  c:no
        #   (3,10,3)  a differs, b == c        (second one, so N=1 != N=2)
        #   (4,40,4)  b differs, a == c        at y=40
        #   (5,20,5)  ALL THREE differ         -> floor a:yes  b:yes  c:yes
        # so floor(a,{b,c}) = {(1,10,1),(3,10,3),(5,20,5)} = 3
        #    floor(b,{a,c}) = {(2,10,2),(4,40,4),(5,20,5)} = 3
        #    floor(c,{a,b}) = {(5,20,5)}                   = 1
        # The block columns are deliberately asymmetric so that reading the
        # wrong direction of a pair would be visible if anything here read
        # blocks. Nothing does, and the symmetric-fallback case proves it.
        P = "{x} {y} {z} minecraft:stone minecraft:dirt\n"
        Q = "{x} {y} {z} minecraft:granite minecraft:andesite\n"
        p1, p2 = P.format(x=1, y=10, z=1), P.format(x=2, y=10, z=2)
        p3, p4 = P.format(x=3, y=10, z=3), P.format(x=4, y=40, z=4)
        p5 = Q.format(x=5, y=20, z=5)
        pos_ab = [p1, p2, p3, p4, p5]   # a != b  (all five)
        pos_ac = [p1, p3, p5]           # a != c
        pos_bc = [p2, p4, p5]           # b != c
        write("b-vs-a.pos", pos_ab)
        write("a-vs-b.pos", pos_ab)
        write("c-vs-a.pos", pos_ac)
        write("a-vs-c.pos", pos_ac)
        write("c-vs-b.pos", pos_bc)
        write("b-vs-c.pos", pos_bc)

        refs = ["a", "b", "c"]
        mat, blocks = load_matrix(str(d), refs, refs, None)
        expect("blocks parsed", blocks, 100)
        expect("floor(a,{b,c})", floor_of(mat, "a", ["b", "c"]), 3)
        expect("floor(b,{a,c})", floor_of(mat, "b", ["a", "c"]), 3)
        expect("floor(c,{a,b})", floor_of(mat, "c", ["a", "b"]), 1)
        expect("floor(c,{a}) N=1", floor_of(mat, "c", ["a"]), 3)
        expect("floor(a,{b}) N=1", floor_of(mat, "a", ["b"]), 5)

        # A floor can only GROW as references are removed / shrink as added.
        expect("monotone in N", floor_of(mat, "a", ["b"]) >= floor_of(
            mat, "a", ["b", "c"]), True)

        # The y band bands, and it bands POSITIVELY: y 15..25 keeps only
        # (5,20,5), which every run differs at, so every floor reads 1. A band
        # test whose every answer is zero would pass on a filter that dropped
        # everything.
        mhi, _ = load_matrix(str(d), refs, refs, (15, 25))
        expect("ybox 15..25 floor(a,{b,c})", floor_of(mhi, "a", ["b", "c"]), 1)
        expect("ybox 15..25 floor(c,{a,b})", floor_of(mhi, "c", ["a", "b"]), 1)
        mlo, _ = load_matrix(str(d), refs, refs, (0, 12))
        expect("ybox 0..12 floor(a,{b,c})", floor_of(mlo, "a", ["b", "c"]), 2)
        expect("ybox 0..12 floor(c,{a,b})", floor_of(mlo, "c", ["a", "b"]), 0)

        # The symmetric fallback: delete one direction, answer must not move.
        (d / "b-vs-a.pos").unlink()
        mat2, _ = load_matrix(str(d), refs, refs, None)
        expect("symmetric fallback", floor_of(mat2, "a", ["b", "c"]), 3)

        # A missing PAIR is a refusal, not a silently smaller ensemble.
        (d / "a-vs-b.pos").unlink()
        try:
            load_matrix(str(d), refs, refs, None)
            expect("missing pair refused", "no raise", "Malformed")
        except Exception as e:  # noqa: BLE001
            expect("missing pair refused", type(e).__name__, "Malformed")

        # A truncated file is a refusal: the differ's own counter is the check.
        (d / "a-vs-b.pos").write_text(
            P.format(x=1, y=10, z=1) + S.format(n=9), encoding="utf-8"
        )
        try:
            load_matrix(str(d), refs, refs, None)
            expect("count mismatch refused", "no raise", "Malformed")
        except Exception as e:  # noqa: BLE001
            expect("count mismatch refused", type(e).__name__, "Malformed")

    # pack() must be injective over the window's real coordinate ranges.
    seen = {}
    for x in (-2048, -1, 0, 1, 4096):
        for z in (-2048, -1, 0, 1, 4096):
            for y in (-64, 0, 319):
                k = pack((x, y, z))
                if k in seen:
                    expect(f"pack collision {(x, y, z)} vs {seen[k]}", False, True)
                seen[k] = (x, y, z)
    expect("pack injective", len(seen), 75)

    print("SELFTEST", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main(argv: list[str]) -> int:
    if "--selftest" in argv:
        return selftest()

    def opt(name: str, default=None):
        return argv[argv.index(name) + 1] if name in argv else default

    posdir = opt("--pos")
    if not posdir or not os.path.isdir(posdir):
        print(f"floor-vs-n.py: --pos DIR required (got {posdir!r})", file=sys.stderr)
        return 2
    refs = (opt("--refs") or "c,d,e,f,g,h,i,j,k,l,m,n,o,p,q,r").split(",")
    ns = [int(x) for x in (opt("--ns") or "2,4,6,8,10,12,14,16").split(",")]
    pools = int(opt("--pools", "40"))
    label = opt("--label", os.path.basename(posdir.rstrip("/")))
    ybox = None
    if opt("--ybox"):
        lo, hi = opt("--ybox").split(",")
        ybox = (int(lo), int(hi))
    rng = random.Random(20260810)

    mat, blocks = load_matrix(posdir, refs, refs, ybox)

    print(f"FLOOR-VS-N {label}")
    print(f"  references            : {len(refs)} ({','.join(refs)})")
    print(f"  window                : {blocks:,} blocks", end="")
    print(f", y band {ybox[0]}..{ybox[1]}" if ybox else ", y full")
    print(f"  pools sampled per n   : up to {pools}")
    print()
    print("  POOL — what a figure at pool n would carry (leave-one-out, N=n-1)")
    print(f"    {'n':>3} {'N':>3}  {'min over pools':>26}  "
          f"{'median':>26}  {'MAX':>26}")
    curve = pool_curve(mat, refs, ns, pools, rng)
    for n, N, mins, meds, maxs in curve:
        print(f"    {n:>3} {N:>3}  {_span(mins):>26}  {_span(meds):>26}  "
              f"{_span(maxs):>26}")

    if "--fixed" in argv:
        fixed = (opt("--fixed") or "c,d,e,f,g,h").split(",")
        print()
        print(f"  FIXED — the same {len(fixed)} candidates at every N "
              f"({','.join(fixed)}), so no extreme-value inflation")
        print(f"    {'N':>3}  {'MAX':>26}  {'median':>26}")
        for N, maxs, meds in fixed_curve(
            mat, refs, fixed, [x for x in ns if x <= len(refs) - 1], pools, rng
        ):
            print(f"    {N:>3}  {_span(maxs):>26}  {_span(meds):>26}")

    if "--per-candidate" in argv:
        # Is a bad reading a property of the RUN or of the draw? This decides
        # whether a bigger pool keeps meeting candidates like `m`, and so
        # whether the max can be extrapolated at all.
        print()
        print("  PER-CANDIDATE — each run's own floor, leave-one-out at three N")
        print(f"    {'cand':>6} {'N=5':>10} {'N=10':>10} {'N=15':>10}")
        rows = []
        for x in refs:
            avail = [r for r in refs if r != x]
            f5 = int(statistics.median(
                floor_of(mat, x, rng.sample(avail, 5)) for _ in range(pools)))
            f10 = int(statistics.median(
                floor_of(mat, x, rng.sample(avail, 10)) for _ in range(pools)))
            f15 = floor_of(mat, x, avail)
            rows.append((f15, x, f5, f10))
        for f15, x, f5, f10 in sorted(rows, reverse=True):
            print(f"    {x:>6} {f5:>10,} {f10:>10,} {f15:>10,}")
        r5 = [r[2] for r in sorted(rows, reverse=True)]
        r15 = [r[0] for r in sorted(rows, reverse=True)]
        # Spearman on ranks, spelled out rather than imported.
        def _rank(v):
            order = sorted(range(len(v)), key=lambda i: v[i])
            rk = [0] * len(v)
            for pos, i in enumerate(order):
                rk[i] = pos
            return rk
        a_, b_ = _rank(r5), _rank(r15)
        n_ = len(a_)
        d2 = sum((a_[i] - b_[i]) ** 2 for i in range(n_))
        rho = 1 - 6 * d2 / (n_ * (n_ * n_ - 1))
        print(f"    rank correlation N=5 vs N=15 (Spearman rho) : {rho:+.3f}")
        print("    rho near 0 => a bad reading is the DRAW, not the run, and a "
              "bigger pool does\n               not keep meeting new bad runs; "
              "near 1 => it is the run, and it does.")

    charge = None
    cpos = opt("--charge-pos")
    if cpos:
        charge = charge_of(cpos, refs, opt("--charge-cand", "ours"), ybox)
        worst = max(max(m) for _, _, _, _, m in curve)
        print()
        print(f"  CHARGE ({os.path.basename(cpos.rstrip('/'))})"
              f"        : {charge:,}")
        print(f"  margin on worst max at n={curve[-1][0]:<3}: "
              f"{charge / max(curve[-1][4]):.2f}x")
        print(f"  margin on median  at n={curve[-1][0]:<3}: "
              f"{charge / statistics.median(curve[-1][3]):.2f}x")
        del worst

    if "--holdout" in argv:
        k = int(opt("--holdout", "8"))
        est, held = holdout(mat, refs, k, pools, rng)
        print()
        print(f"  HOLDOUT (§18.7) estimate on {k}, test on {len(refs) - k}")
        print(f"    estimating half max : {_span(est)}")
        print(f"    HELD-OUT half max   : {_span(held)}")
        if max(held) > max(est):
            print("    -> the held-out candidates are charged MORE than the "
                  "estimating half. Selection on the estimate.")

    if "--selfcheck-floor" in argv:
        vals = [floor_of(mat, x, [r for r in refs if r != x]) for x in refs]
        got = (min(vals), int(statistics.median(vals)), max(vals))
        want = (141, 590, 2241)
        if ybox is not None:
            print("\n  selfcheck-floor skipped: a y band is not the "
                  "registered full-y figure.")
        elif got != want:
            raise Refuse(
                f"pool {len(refs)} N={len(refs) - 1} reads min/med/max {got}, "
                f"the registered ow-r16-12345-level-n15-iceberg floor is {want}. "
                "The arithmetic or the reference set is not the one that figure "
                "was taken on."
            )
        else:
            print(f"\n  selfcheck-floor OK    : {got} == registered {want}")

    if opt("--selfcheck-charge"):
        want = int(opt("--selfcheck-charge"))
        if charge is None:
            raise Refuse("--selfcheck-charge needs --charge-pos")
        if ybox is not None:
            print("  selfcheck-charge skipped: a y band is not the registered "
                  "full-y figure.")
        elif charge != want:
            raise Refuse(
                f"charge reads {charge:,}, registered figure is {want:,}. "
                "Either the .pos directory is not the tip that figure was "
                "taken on, or the arithmetic is wrong."
            )
        else:
            print(f"  selfcheck-charge OK   : {charge:,} == registered {want:,}")

    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Refuse as exc:
        print(f"REFUSED: {exc}", file=sys.stderr)
        sys.exit(3)
    except Exception as exc:  # noqa: BLE001
        print(f"floor-vs-n.py: {exc}", file=sys.stderr)
        sys.exit(2)
