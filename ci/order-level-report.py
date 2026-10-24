#!/usr/bin/env python3
"""order-level-report.py — the r=16 overworld LEVEL, with the harvest order as
an explicit column, and the innocent-vanilla floor beside every charge.

# WHAT IT ANSWERS

`docs/DETERMINISM.md` §15 established that the published level
(`ci/figures.toml:ow-r16-admissible-n2`, 29,605 of 107,053,056) was taken with
our dump walking chunks x-major against references harvested z-major, and that
the same instrument charges an *unmodified vanilla run* 117,628 blocks for that
mismatch. So the level has to be re-taken with the order controlled, and there
are two ways to control it:

    A  MATCH   dump order = the references' harvest order (`zx`)
    B  WIDEN   ensemble holds both harvest orders, dump stays at the default

A controls the variable; B averages over it. They are different questions and
this prints both, at every N, each against the innocent floor **taken from the
same ensemble at the same N** — because a floor belongs to the ensemble that
produced it and, since §15, to its harvest order too.

# THE FLOOR IS NOT OPTIONAL AND IT IS NOT A SUBTRACTION

`floor(N)` here is the charge the instrument lays on a *known-innocent*
candidate: a genuine unmodified vanilla run substituted for our dump and
adjudicated against N of the others. It is a positive control. It is reported
beside the charge, never subtracted from it — §12 subtracted a twin-mask width
from an unmasked charge, called the difference "45 blocks of headroom", and
that category error cost a brief.

A charge BELOW its floor is not a better result than a charge above it. It
means the instrument can no longer tell the two candidates apart, and the
honest word for a number in that regime is corroboration.

# p_hide, AND WHY EVERY RUNG CARRIES IT

An ensemble of N references can only catch a defect that at least one of them
declines to hide. If each reference independently hides it with probability p,
all N hide it with probability p^N, and

    p_hide(N) = 1 - alpha**(1/N)        alpha = 0.05

is the largest p at which N references still have 95% power. N=3 hides 63.2%,
N=6 hides 39.3%, N=10 hides 25.9%. A rung without its N is a rung whose
resolving power is unstated, which is how "the End's floor is 0" was published
off one lucky pair.

usage:
  order-level-report.py --pos <dir> --rows a,b,... --rrev rr-a,... \
                        [--denominator 107053056] [--max-subsets 40]
  order-level-report.py --selftest
exit 0 fine, 2 malformed or missing inputs.
"""

from __future__ import annotations

import importlib.util
import itertools
import os
import statistics
import sys

ALPHA = 0.05
_HERE = os.path.dirname(os.path.abspath(__file__))
_PARSER = None


def _parser():
    """Borrow `load()` from admissible-residual.py — anchored, and it refuses
    unless the line count equals the differ's own `mismatches=`.

    Cached deliberately: loading the module twice makes two distinct
    `Malformed` classes and an `except` in one place stops catching a `raise`
    from the other.
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


def p_hide(n: int, alpha: float = ALPHA) -> float:
    return 1.0 - alpha ** (1.0 / n)


def charge(files, subset, cand: str) -> int:
    """|{p : cand's block is in no reference of `subset`}|.

    Each `<ref>-vs-<cand>.pos` lists exactly the positions where that reference
    differs from the candidate, so a position charged by every reference in the
    subset is one where the candidate's block matches none of them.
    """
    sets = [files[(r, cand)] for r in subset]
    # `sets[0].intersection(...)` rather than `set.intersection(*sets)`: the
    # loader hands back frozensets and the unbound-method form rejects them.
    # The self-test used plain sets and so could not see it — it now uses
    # frozensets, which is the whole reason this comment is here.
    return len(sets[0].intersection(*sets[1:]))


def exactness(residual: int, denom: int) -> str:
    return f"{100.0 * (denom - residual) / denom:.6f}%"


def floor_spread(files, pool, n: int, max_subsets: int):
    """Charge an innocent vanilla candidate against n of the others.

    Every member of `pool` takes its turn as the candidate, which is what makes
    this a positive control rather than a single lucky substitution.
    """
    vals = []
    for cand in pool:
        others = [r for r in pool if r != cand]
        if len(others) < n:
            continue
        combos = list(itertools.combinations(others, n))
        if len(combos) > max_subsets:
            step = len(combos) / max_subsets
            combos = [combos[int(i * step)] for i in range(max_subsets)]
        for s in combos:
            if all((r, cand) in files for r in s):
                vals.append(charge(files, list(s), cand))
    return vals


def load_needed(posdir: str, refs, cands):
    ar = _parser()
    out = {}
    blocks = chunks = None
    for cand in cands:
        for ref in refs:
            if ref == cand:
                continue
            p = os.path.join(posdir, f"{ref}-vs-{cand}.pos")
            if not os.path.exists(p):
                continue
            pos, b, c = ar.load(p)
            # KEYS ONLY, AND ONCE. `load()` hands back a dict position ->
            # (their block, ours); nothing here reads the values, and rebuilding
            # a set from the dict inside every intersection turned a minute of
            # set algebra into an hour of copying. Measured 2026-08-10: the
            # floor spread alone is ~13,000 intersections over sets of up to
            # 150,000 positions.
            pos = frozenset(pos)
            if blocks is None:
                blocks, chunks = b, c
            elif (b, c) != (blocks, chunks):
                raise ar.Malformed(
                    f"{p} covers {c} chunks / {b:,} blocks against "
                    f"{chunks} / {blocks:,}. One window, or every intersection "
                    "below is meaningless."
                )
            out[(ref, cand)] = pos
    if blocks is None:
        raise ar.Malformed(f"no position files loaded from {posdir}")
    return out, blocks, chunks


def run(posdir: str, rows, rrev, denom: int, max_subsets: int,
        seed: str = "20260722", known_rows=("i",), matched: str = "zx") -> int:
    ar = _parser()
    pool = rows + rrev
    cands = ["ours", matched] + pool
    files, blocks, chunks = load_needed(posdir, pool, cands)
    if blocks != denom:
        print(
            f"  NOTE: the .pos files cover {blocks:,} blocks and the "
            f"denominator given is {denom:,}. Using the files' own figure.",
        )
        denom = blocks
    print(f"window: {chunks} chunks, {blocks:,} blocks, seed {seed} r=16 c(64,64)")
    print(f"pool:   {len(rows)} rows-harvested {rows} + {len(rrev)} rows_rev {rrev}")
    print()

    # ---- the two ways to control the order, at every N they can be taken at --
    arms = [
        ("A match   ", matched, rows, "rows"),
        ("B widen   ", "ours", pool, "rows+rows_rev"),
        ("old uncontrolled", "ours", rows, "rows"),
        ("B' widen+match  ", matched, pool, "rows+rows_rev"),
    ]
    print(
        f"  {'arm':16} {'dump':5} {'harvest':14} {'N':>2}  {'charge':>9}  "
        f"{'exactness':>12}  {'floor min':>9} {'med':>9} {'max':>9}  {'p_hide':>6}"
    )
    for label, cand, ens, harvest in arms:
        for n in range(2, len(ens) + 1):
            sub = ens[:n]
            if not all((r, cand) in files for r in sub):
                continue
            c = charge(files, sub, cand)
            fl = floor_spread(files, ens, n, max_subsets)
            # NO FLOOR IS NOT A FLOOR OF ZERO. At N = |pool| every member is
            # needed as a reference and none is left over to be the innocent
            # candidate, so there is no floor to take — and printing 0 there
            # would read as "this instrument has no noise", which is exactly
            # how "the End's floor is 0" got published off one lucky pair
            # (docs/DETERMINISM.md §5). It prints as `—` and carries no mark.
            if fl:
                cells = f"{min(fl):>9,} {int(statistics.median(fl)):>9,} {max(fl):>9,}"
                mark = "  <- BELOW FLOOR" if c < min(fl) else (
                    "  <- inside floor" if c < statistics.median(fl) else "")
            else:
                cells = f"{'—':>9} {'—':>9} {'—':>9}"
                mark = "  <- no floor at this N: every reference is in the ensemble"
            print(
                f"  {label:16} {cand:5} {harvest:14} {n:>2}  {c:>9,}  "
                f"{exactness(c, denom):>12}  {cells}  "
                f"{p_hide(n) * 100:>5.1f}%{mark}"
            )
        print()

    # ---- classify each reference's harvest order from the matrix ------------
    #
    # a..h are recorded nowhere as `rows`; §15 asserts it because that is how
    # `ci/gen-vanilla-region.sh` was always driven. An assertion about how a
    # tool was driven is exactly the kind of thing this project keeps finding to
    # be false, and it does not have to be an assertion: two vanilla runs of the
    # same order differ by ~32,000 blocks on this window and two of different
    # orders by ~150,000, which is a 4.6x gap with nothing in it. So the order
    # is READ OFF the matrix, and only then written into a ledger.
    # `known_rows` is the anchor: a reference whose harvest order is not in
    # doubt, against which every other member is scored. On the 20260722
    # ensemble that is `i`, the only member generated after the stamp existed.
    # It is an argument rather than a literal because a second seed has a
    # different set of anchors, and hardcoding `i` made this whole block
    # silently skip on any ensemble that has no member called `i` — a
    # classification that does not run reads exactly like one that agreed.
    known_rows = [r for r in pool if r in set(known_rows)]
    known_rrev = list(rrev)
    if known_rows and known_rrev:
        print()
        print("  harvest order read off the matrix, not asserted:")
        print(f"    {'ref':6} {'median vs known-rows':>21} {'median vs known-rows_rev':>25}  verdict")
        for r in pool:
            a = [len(files[(r, k)]) for k in known_rows if (r, k) in files]
            b = [len(files[(r, k)]) for k in known_rrev if (r, k) in files]
            if not a or not b:
                continue
            ma, mb = statistics.median(a), statistics.median(b)
            verdict = "rows" if ma < mb else "rows_rev"
            print(f"    {r:6} {ma:>21,.0f} {mb:>25,.0f}  {verdict}"
                  f"{'   <- ' + str(round(max(ma, mb) / max(min(ma, mb), 1), 1)) + 'x' if a and b else ''}")

    # ---- the order-invariant core: what neither arm can explain away ---------
    both = None
    for r in pool:
        if (matched not in cands) or (r, matched) not in files or (r, "ours") not in files:
            continue
        s = set(files[(r, "ours")]) & set(files[(r, matched)])
        both = s if both is None else (both & s)
    if both is not None:
        print(f"  charged under BOTH dump orders against all {len(pool)}: {len(both):,}")
    return 0


def selftest() -> int:
    """Two properties that would make every number above wrong if broken."""
    ok = True
    # p_hide is the inverse of the power statement it claims to be.
    for n, want in ((3, 0.632), (6, 0.393), (10, 0.259)):
        got = p_hide(n)
        if abs(got - want) > 5e-4:
            print(f"  FAIL p_hide({n}) = {got:.4f}, want {want}")
            ok = False
    # A charge is an intersection, so adding a reference can only shrink it.
    files = {
        ("a", "x"): frozenset({1, 2, 3}),
        ("b", "x"): frozenset({2, 3, 4}),
        ("c", "x"): frozenset({3, 4, 5}),
    }
    seq = [charge(files, ["a"], "x"), charge(files, ["a", "b"], "x"),
           charge(files, ["a", "b", "c"], "x")]
    if seq != [3, 2, 1]:
        print(f"  FAIL monotone charge: {seq}")
        ok = False
    # The floor uses every pool member as the candidate in turn; with three
    # references that is three candidates, each against the other two at N=2.
    fl = floor_spread({k: frozenset(v) for k, v in
                       {("a", "b"): {1}, ("c", "b"): {1, 2},
                        ("b", "a"): {7}, ("c", "a"): {8},
                        ("a", "c"): {9}, ("b", "c"): {9}}.items()},
                      ["a", "b", "c"], 2, 99)
    if sorted(fl) != [0, 1, 1]:
        print(f"  FAIL floor_spread: {sorted(fl)}")
        ok = False
    print("  order-level-report selftest", "OK" if ok else "FAILED")
    return 0 if ok else 1


def main(argv) -> int:
    if argv[:1] == ["--selftest"]:
        return selftest()
    opts = {}
    i = 0
    while i < len(argv):
        if argv[i].startswith("--") and i + 1 < len(argv):
            opts[argv[i]] = argv[i + 1]
            i += 2
            continue
        print(f"unexpected argument {argv[i]}", file=sys.stderr)
        return 2
    if "--pos" not in opts or "--rows" not in opts:
        print(
            "usage: order-level-report.py --pos <dir> --rows a,b,... "
            "[--rrev rr-a,...] [--denominator N] [--max-subsets N] "
            "[--seed S] [--known-rows a,b] [--matched-label zx]",
            file=sys.stderr,
        )
        return 2
    rows = [r for r in opts["--rows"].split(",") if r]
    rrev = [r for r in opts.get("--rrev", "").split(",") if r]
    ar = _parser()
    try:
        return run(
            opts["--pos"],
            rows,
            rrev,
            int(opts.get("--denominator", "107053056")),
            int(opts.get("--max-subsets", "40")),
            opts.get("--seed", "20260722"),
            tuple(r for r in opts.get("--known-rows", "i").split(",") if r),
            opts.get("--matched-label", "zx"),
        )
    except (ar.Malformed, OSError) as exc:
        print(f"LEVEL-REPORT MALFORMED: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
