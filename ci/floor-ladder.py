#!/usr/bin/env python3
"""floor-ladder.py — `docs/DETERMINISM.md` §4's ensemble ladder, re-taken with
the harvest order on every rung, and the section's standing claim tested.

# WHY THIS EXISTS RATHER THAN ANOTHER RUN OF ci/floor-ensemble.sh

§4's ladder was declared un-re-takeable on 2026-08-09: its six references were
lost in the disk failure. Eight overworld references exist again, plus three
order-varied ones, and the ladder needs **no new diffs at all** — every rung is
a set operation on the `<r1>-vs-<r2>.pos` matrix `ci/order-level-retake.sh`
already writes. With `--twin refj` dropping the positions where ref1 and refj
disagree,

    floor_N     = union over j>=2 of  pos(ref1, refj)
    residual_N  = pos(ref1, ours) \\ floor_N
    denom_N     = blocks - floor_N

which is the definition in `ci/floor-ensemble.sh`'s header, rearranged. A
registered figure is the self-check and the run REFUSES if it misses it:
`ow-r16-floor-n2` = 29,560 must fall out at N=2 on references a,b.

**Only the floor can be a refusal, and the reason is worth more than the
check.** The obvious second check is the twin-masked residual, 29,490 — and it
is the wrong one, because the residual reads OUR dump and the tree has moved
since `main` 8e283f57 where that figure was taken (`fluid.rs`,
`post_process.rs`, `beard.rs` and `chunk_gen.rs` all changed; the lava-to-
obsidian cascade alone moved window obsidian 20 -> 46). A residual that failed
to reproduce would be the branch working, not the arithmetic breaking, and
wiring it as a refusal would have made a correct run abort. The floor is
vanilla against vanilla and reads no code of ours at all, so it must reproduce
to the block on any tree — which is exactly what makes it the honest check. The
residual is printed beside its registered value as a **delta**, which is sound
on a fixed reference set and is the only thing the two numbers may be read as.

# WHAT EVERY RUNG NOW CARRIES, AND WHY

* **N** — a floor is the size of a union and can only grow with N, so a floor
  without its N is a different and false claim, not a weaker one.
* **the harvest order of the references** — §15: two unmodified vanilla runs of
  one seed and window disagree by 146,169 blocks across chunk orders and 32,127
  within one. A floor quoted with its N but not its harvest order is off by up
  to 4.6x, and the ladder is the place that error would compound silently.
* **p_hide(N) = 1 - alpha**(1/N)**, alpha = 0.05 — the largest per-reference
  probability of hiding a defect at which N references still have 95% power.
  N=3 hides 63.2%, N=6 hides 39.3%, N=10 hides 25.9%.

# THE STANDING CLAIM THIS TESTS

§4 says: *"The floors do not move with our code at all, only with the reference
set."* `--claim` tests both halves.

* Hold the reference set fixed and vary our code — here, two dumps of the same
  tree under different chunk orders, which is the largest swing our own code
  has available. If the floor moves, the claim's first half is false and so is
  every floor row in the section.
* Hold N fixed and vary what the reference set *is*. The claim's second half is
  true, and stated that way it is useless: it reads as "floors move with how
  many references you have", and the number that actually dominates is not the
  count but the harvest order.

usage:
  floor-ladder.py --pos <dir> --rows a,b,... [--rrev rr-a,...]
                  [--ours ours,zx] [--claim] [--max-subsets 60]
                  [--selfcheck-floor N]
  floor-ladder.py --selftest
exit 0 fine, 2 malformed input, 3 a self-check that did not reproduce.
"""

from __future__ import annotations

import importlib.util
import itertools
import os
import statistics
import sys

ALPHA = 0.05
# Taken by diffing, registered in ci/figures.toml, on references a,b with the
# x-major dump. They are the only thing standing between this file and a
# plausible reconstruction of a number nobody measured.
#
# THEY BELONG TO ONE WINDOW AND THE FIRST VERSION DID NOT SAY SO. Run against
# the nether ensemble the check fired — 377,949 against 29,560 — and refused a
# run that was perfectly correct, because the nether's floor is not the
# overworld's and never was. Keyed on the block count, which identifies the
# window, with `--selfcheck-floor` to supply one for any other window. A window
# with no registered floor gets a LOUD "no self-check" line rather than a
# silent skip: an unchecked reconstruction that looks checked is worse than one
# that admits it is not.
SELFCHECK_BY_BLOCKS = {107053056: (29560, 29490)}

_HERE = os.path.dirname(os.path.abspath(__file__))
_PARSER = None


def _parser():
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


def floor_of(files, primary: str, others) -> set:
    """Positions the ensemble is not unanimous on, seen from `primary`."""
    acc: set = set()
    for r in others:
        acc |= files[(primary, r)]
    return acc


def rung(files, primary: str, others, cand: str, blocks: int):
    """floor, residual and denominator. DELIBERATELY NO PERCENTAGE.

    The denominator is the window less the floor, so it shrinks as N grows and
    an exactness computed from it improves partly because the mask grew. §4 has
    said "quote the block count, not the percentage" since it was written, and
    the first version of this function printed the percentage anyway — nine
    rungs of it, which `ci/check-figures.py` then correctly refused as nineteen
    unregistered figures. A number that must not be quoted is best not
    produced.
    """
    fl = floor_of(files, primary, others)
    res = files[(primary, cand)] - fl
    return len(fl), len(res), blocks - len(fl)


def load_matrix(posdir: str, refs, cands):
    ar = _parser()
    out, blocks, chunks = {}, None, None
    for cand in list(refs) + list(cands):
        for ref in refs:
            if ref == cand:
                continue
            p = os.path.join(posdir, f"{ref}-vs-{cand}.pos")
            if not os.path.exists(p):
                continue
            pos, b, c = ar.load(p)
            # Keys only, and once — see ci/order-level-report.py's load for the
            # measurement that made this necessary rather than tidy.
            pos = frozenset(pos)
            if blocks is None:
                blocks, chunks = b, c
            elif (b, c) != (blocks, chunks):
                raise ar.Malformed(
                    f"{p} covers {c} chunks / {b:,} blocks against {chunks} / "
                    f"{blocks:,}; one window or every union below is nonsense"
                )
            out[(ref, cand)] = pos
    if blocks is None:
        raise ar.Malformed(f"no position files loaded from {posdir}")
    return out, blocks, chunks


def ladder(files, pool, harvest: str, cand: str, blocks: int, label: str):
    print(f"  {label}   references harvested {harvest}, candidate dump `{cand}`")
    print(
        f"    {'N':>2}  {'floor':>9}  {'residual':>9}  {'denominator':>13}  "
        f"{'p_hide':>7}"
    )
    rows = []
    for n in range(2, len(pool) + 1):
        sub = pool[:n]
        if not all((sub[0], r) in files for r in sub[1:]):
            break
        if (sub[0], cand) not in files:
            break
        f, r, d = rung(files, sub[0], sub[1:], cand, blocks)
        rows.append((n, f, r, d))
        print(f"    {n:>2}  {f:>9,}  {r:>9,}  {d:>13,}  {p_hide(n) * 100:>6.1f}%")
    print()
    return rows


def claim(files, rows_pool, rrev_pool, blocks: int, max_subsets: int) -> None:
    print("== §4's standing claim: 'the floors do not move with our code at all,")
    print("   only with the reference set'")
    print()
    print("-- half one: hold the references fixed, vary our code.")
    print("   Two dumps of one tree under different chunk orders is the biggest")
    print("   swing our own code has; the charge between them is ~200k blocks.")
    same = True
    for n in range(2, len(rows_pool) + 1):
        sub = rows_pool[:n]
        f = len(floor_of(files, sub[0], sub[1:]))
        line = f"   N={n:<2} floor={f:>9,}"
        vals = []
        for cand in ("ours", "zx"):
            if (sub[0], cand) in files:
                vals.append(len(files[(sub[0], cand)] - floor_of(files, sub[0], sub[1:])))
        if len(vals) == 2:
            line += f"   residual under the two dumps: {vals[0]:>9,} vs {vals[1]:>9,}"
        print(line)
    print("   The floor column does not read our dump at all — it is a union over")
    print("   reference pairs — so this half is TRUE by construction, and the")
    print("   residual column moving while the floor does not is the positive")
    print("   control that says the two are really being computed separately.")
    print()
    print("-- half two: hold N fixed and vary WHAT the reference set is.")
    print("   The claim credits the reference set and says nothing about which")
    print("   property of it. At fixed N the dominant property is not the count.")
    for n in (2, 3, 4):
        rows_vals, mixed_vals = [], []
        for combo in itertools.combinations(rows_pool, n):
            rows_vals.append(len(floor_of(files, combo[0], combo[1:])))
        if rrev_pool:
            pool = rows_pool + rrev_pool
            for combo in itertools.combinations(pool, n):
                if not any(r in rrev_pool for r in combo):
                    continue
                if not all(r in rrev_pool for r in combo):
                    mixed_vals.append(len(floor_of(files, combo[0], combo[1:])))
        if len(rows_vals) > max_subsets:
            rows_vals = rows_vals[:max_subsets]
        if len(mixed_vals) > max_subsets:
            mixed_vals = mixed_vals[:max_subsets]
        def s(v):
            return (f"min {min(v):>9,} med {int(statistics.median(v)):>9,} "
                    f"max {max(v):>9,}  (n={len(v)})") if v else "none"
        print(f"   N={n}  one harvest order : {s(rows_vals)}")
        print(f"        mixed harvest orders: {s(mixed_vals)}")
        if rows_vals and mixed_vals:
            print(f"        ratio of medians    : "
                  f"{statistics.median(mixed_vals) / statistics.median(rows_vals):.2f}x")
    print()


def run(posdir, rows_pool, rrev_pool, cands, do_claim, max_subsets,
        selfcheck_floor=None) -> int:
    files, blocks, chunks = load_matrix(posdir, rows_pool + rrev_pool, cands)
    print(f"window: {chunks} chunks, {blocks:,} blocks, seed 20260722 r=16 c(64,64)")
    print()
    # -- the self-check, before anything derived is printed ------------------
    want = SELFCHECK_BY_BLOCKS.get(blocks) if selfcheck_floor is None else (
        selfcheck_floor, None)
    if ("a", "b") not in files:
        print("!! NO SELF-CHECK: no a-vs-b in the matrix")
    elif want is None:
        print(
            f"!! NO SELF-CHECK for a {blocks:,}-block window: no registered "
            "floor is known for it and none was given with --selfcheck-floor. "
            "Everything below is a reconstruction that has not been checked "
            "against anything measured by diffing."
        )
    else:
        f = len(floor_of(files, "a", ["b"]))
        ok = f == want[0]
        print(
            f"selfcheck  floor N=2 on a,b: {f:,} (registered {want[0]:,}) — "
            f"{'OK' if ok else 'MISMATCH'}"
        )
        if not ok:
            print(
                "REFUSING: the floor is vanilla against vanilla and reads no "
                "code of ours, so it must reproduce to the block on any tree. "
                "It did not. Nothing below is trustworthy.",
                file=sys.stderr,
            )
            return 3
        if want[1] is not None and ("a", "ours") in files:
            r = len(files[("a", "ours")] - floor_of(files, "a", ["b"]))
            print(
                f"           twin-masked residual N=2, x-major dump: {r:,} "
                f"against {want[1]:,} at main 8e283f57 — delta "
                f"{r - want[1]:+,} on a fixed reference pair. NOT a "
                f"self-check: the tree has moved and this is what that cost."
            )
    print()
    for cand in cands:
        if any((r, cand) in files for r in rows_pool):
            ladder(files, rows_pool, "rows", cand, blocks, "§4 ladder")
    if rrev_pool:
        for cand in cands:
            pool = rows_pool + rrev_pool
            if any((r, cand) in files for r in pool):
                ladder(files, pool, "rows then rows_rev", cand, blocks,
                       "§4 ladder, mixed harvest")
    if do_claim:
        claim(files, rows_pool, rrev_pool, blocks, max_subsets)
    return 0


def selftest() -> int:
    ok = True
    for n, want in ((3, 0.632), (6, 0.393), (10, 0.259)):
        if abs(p_hide(n) - want) > 5e-4:
            print(f"  FAIL p_hide({n})={p_hide(n):.4f} want {want}")
            ok = False
    # residual = our mismatches minus the floor, and the floor is a union that
    # can only grow. Position 3 is inside the floor at N=3 and must drop out.
    # Frozensets, matching what the loader really hands `rung` — a self-test
    # over plain sets let a `set.intersection` type error through to the box.
    files = {
        ("a", "b"): frozenset({1, 2}),
        ("a", "c"): frozenset({3}),
        ("a", "ours"): frozenset({2, 3, 4, 5}),
    }
    f2, r2, d2 = rung(files, "a", ["b"], "ours", 100)
    f3, r3, d3 = rung(files, "a", ["b", "c"], "ours", 100)
    if (f2, r2, d2) != (2, 3, 98) or (f3, r3, d3) != (3, 2, 97):
        print(f"  FAIL rung: N=2 {(f2, r2, d2)} N=3 {(f3, r3, d3)}")
        ok = False
    print("  floor-ladder selftest", "OK" if ok else "FAILED")
    return 0 if ok else 1


def main(argv) -> int:
    if argv[:1] == ["--selftest"]:
        return selftest()
    opts, flags = {}, set()
    i = 0
    while i < len(argv):
        if argv[i] == "--claim":
            flags.add("claim")
            i += 1
            continue
        if argv[i].startswith("--") and i + 1 < len(argv):
            opts[argv[i]] = argv[i + 1]
            i += 2
            continue
        print(f"unexpected argument {argv[i]}", file=sys.stderr)
        return 2
    if "--pos" not in opts or "--rows" not in opts:
        print(
            "usage: floor-ladder.py --pos <dir> --rows a,b,... [--rrev rr-a,...] "
            "[--ours ours,zx] [--claim] [--selfcheck-floor N]",
            file=sys.stderr,
        )
        return 2
    ar = _parser()
    try:
        return run(
            opts["--pos"],
            [r for r in opts["--rows"].split(",") if r],
            [r for r in opts.get("--rrev", "").split(",") if r],
            [c for c in opts.get("--ours", "ours,zx").split(",") if c],
            "claim" in flags,
            int(opts.get("--max-subsets", "60")),
            int(opts["--selfcheck-floor"]) if "--selfcheck-floor" in opts else None,
        )
    except (ar.Malformed, OSError) as exc:
        print(f"FLOOR-LADDER MALFORMED: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
