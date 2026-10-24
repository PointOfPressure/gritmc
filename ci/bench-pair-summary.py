#!/usr/bin/env python3
"""Summarise a ci/bench-pair.sh TSV as a PAIRED ratio with a confidence
interval, for every metric the run captured.

The interval is the point of this script. `ci/bench-servers.sh` handed a
change the wrong sign twice because an unpaired difference was read as a
result; a paired ratio whose interval straddles 1.0 is *unresolved*, and
saying so is a different finding from "no change". Both are printed, and the
script refuses to call a verdict it cannot support.

Pairs are formed on (rep, centre), so the 7-17% between-centre terrain spread
cancels exactly rather than being averaged over.

INSTRUCTIONS is the primary metric where present: it measures work done rather
than time taken, so it is not inflated by whatever else holds the box. CPU-s
is reported beside it because CPU-s/chunk is the quantity docs/BENCH.md
speaks in. Where the two disagree, the difference is the contention.
"""
import sys
import math
from collections import defaultdict
from statistics import mean, stdev

T95 = {1: 12.706, 2: 4.303, 3: 3.182, 4: 2.776, 5: 2.571, 6: 2.447,
       7: 2.365, 8: 2.306, 9: 2.262, 10: 2.228, 11: 2.201, 12: 2.179,
       13: 2.160, 14: 2.145, 15: 2.131, 16: 2.120, 17: 2.110, 18: 2.101,
       19: 2.093, 20: 2.086, 21: 2.080, 22: 2.074, 23: 2.069, 24: 2.064,
       25: 2.060, 26: 2.056, 27: 2.052, 28: 2.048, 29: 2.045, 30: 2.042}


def tcrit(df):
    return float("nan") if df <= 0 else T95.get(df, 1.96)


def paired(cell, xn, yn, metric):
    out = []
    for (rep, centre, arm, m), v in cell.items():
        if arm != xn or m != metric:
            continue
        other = cell.get((rep, centre, yn, metric))
        if other is None or v <= 0 or other <= 0:
            continue
        out.append((rep, centre, v, other, other / v))
    out.sort(key=lambda p: (int(p[0]), p[1]))
    return out


def report(pairs, xn, yn, metric, unit, per_chunk=None):
    if len(pairs) < 2:
        print(f"\n{metric}: UNRESOLVED — fewer than two complete pairs")
        return
    print(f"\n--- {metric} ---")
    print(f"{'rep':>4}  {'centre':<11}{xn:>16}{yn:>16}{'ratio':>9}")
    for rep, centre, cx, cy, r in pairs:
        print(f"{rep:>4}  {centre:<11}{cx:>16,.0f}{cy:>16,.0f}{r:>9.4f}"
              if unit == "count" else
              f"{rep:>4}  {centre:<11}{cx:>16.2f}{cy:>16.2f}{r:>9.4f}")

    logs = [math.log(p[4]) for p in pairs]
    n = len(logs)
    gm = math.exp(mean(logs))
    sd = stdev(logs) if n > 1 else 0.0
    se = sd / math.sqrt(n)
    half = tcrit(n - 1) * se
    lo, hi = math.exp(mean(logs) - half), math.exp(mean(logs) + half)
    mde = (math.exp(tcrit(n - 1) * se) - 1) * 100

    if per_chunk:
        mx = mean([p[2] for p in pairs]) / per_chunk
        my = mean([p[3] for p in pairs]) / per_chunk
        print(f"\nper delivered chunk: {xn} {mx:,.6f}   {yn} {my:,.6f}"
              if unit != "count" else
              f"\nper delivered chunk: {xn} {mx:,.0f}   {yn} {my:,.0f}")

    # The pooled ratio weights by work rather than by square, which is the
    # right statistic for "what did this cost overall"; the geometric mean of
    # per-pair ratios answers "what did it cost at a typical centre". They
    # differ whenever the effect is lumpy across centres, which for a
    # spacing-grid structure it must be.
    sx, sy = sum(p[2] for p in pairs), sum(p[3] for p in pairs)
    dmean = mean([p[3] - p[2] for p in pairs])
    print(f"\npooled ratio (sum {yn} / sum {xn}): {sy/sx:.4f} "
          f"({(sy/sx-1)*100:+.2f}%)")
    if unit == "count":
        print(f"mean ABSOLUTE delta per square: {dmean:>18,.0f}")
    else:
        print(f"mean ABSOLUTE delta per square: {dmean:>18,.2f}")
    print(f"  -> constant across radii = a FIXED (per-process) cost;")
    print(f"     scaling with generated area = a PER-CHUNK cost.")
    print(f"PAIRED RATIO {yn}/{xn}: {gm:.4f}  ({(gm-1)*100:+.2f}%)")
    print(f"95% CI: {lo:.4f} .. {hi:.4f}  "
          f"({(lo-1)*100:+.2f}% .. {(hi-1)*100:+.2f}%)   n={n} pairs")
    print(f"resolving power at this n: +/-{mde:.2f}%")
    if lo <= 1.0 <= hi:
        print(f"VERDICT: UNRESOLVED — the interval straddles 1.0. Not a null; "
              f"an effect below +/-{mde:.2f}%, or too small an n to see it.")
    else:
        print(f"VERDICT: {yn} is {'SLOWER/MORE' if gm > 1 else 'FASTER/LESS'} "
              f"than {xn} by {abs(gm-1)*100:.2f}% (CI excludes 1.0).")


def main():
    path, xn, yn, chunks = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
    cell, shas, contend = {}, {}, []
    by_arm = defaultdict(lambda: defaultdict(list))
    with open(path) as fh:
        for line in fh:
            if line.startswith("#") or line.startswith("rep\t"):
                continue
            f = line.rstrip("\n").split("\t")
            if len(f) < 9:
                continue
            rep, centre, arm, sha = f[0], f[1], f[2], f[3]
            cpu, wall, ins, cyc, cb = (float(f[4]), float(f[5]),
                                       float(f[6]), float(f[7]), int(f[8]))
            shas[arm] = sha
            contend.append(cb)
            for m, v in (("cpu_s", cpu), ("wall_s", wall),
                         ("instructions", ins), ("cycles", cyc)):
                cell[(rep, centre, arm, m)] = v
                by_arm[arm][m].append(v)

    if not cell:
        print("no data rows")
        return 2

    print(f"arms: {xn}={shas.get(xn,'?')}  {yn}={shas.get(yn,'?')}")
    print(f"denominator: {chunks} chunks delivered per square")
    if contend:
        bad = sum(1 for c in contend if c > 0)
        print(f"contention: {bad}/{len(contend)} runs saw competing processes"
              f" (max {max(contend)})")
        if bad:
            print("  -> CPU-s is contaminated to that extent; instructions is not.")

    have_ins = any(v > 0 for (_, _, _, m), v in cell.items()
                   if m == "instructions")
    if have_ins:
        report(paired(cell, xn, yn, "instructions"), xn, yn,
               "instructions retired (PRIMARY — immune to contention)",
               "count", chunks)
    else:
        print("\ninstructions: NOT CAPTURED (perf unavailable) — "
              "the contention-immune column is absent, not zero.")
    report(paired(cell, xn, yn, "cpu_s"), xn, yn,
           "CPU-seconds (secondary — the docs/BENCH.md quantity)",
           "time", chunks)
    return 0


if __name__ == "__main__":
    sys.exit(main())
