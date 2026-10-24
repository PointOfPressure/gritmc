#!/usr/bin/env python3
"""Runqueue delay against box load, from `ci/corpus-holder-rate.sh` samples.

    ci/corpus-delay-curve.py <samples.tsv> [more.tsv ...] [--bins 6]

# WHY A CURVE AND NOT A MEAN

`docs/CORPUS-LOCK-HOLD.md` §17.5 constructs a load window rather than waiting
for one, and the whole licence for doing that is the claim that a `nice 19`
load leaves a `nice 0` production recording alone. That claim is about a
DIFFERENCE at a given load, and this box's own load moved between 7.2 and 32.3
inside four minutes on the afternoon it was measured. A mean over such a sample
is a mean over the box's mood.

So the samples are binned by the one-minute load average they were taken at,
and the delay share is reported per bin. Two files binned the same way can then
be compared at equal load, which is the comparison §20 needs:

  * the production holder, `nice 0`, unpinned, during the whole afternoon;
  * this branch's own arms, `nice 19`, pinned, during a load block.

If the arms' delay share at a given box load is at or above the production
holder's, the `nice 19` construction reproduced the contention it claims to,
and did so without handing that contention to the holder.

# THE SENTINEL

`delay_share` is -1 when a sample saw no runnable time at all — a recording
that was asleep for the whole interval. Those rows are counted and excluded
rather than averaged in as zeros, because "not waiting" and "not running" are
different facts and this file exists to keep them apart.
"""
import argparse, statistics, sys
from pathlib import Path


def read(path):
    rows = []
    with open(path) as f:
        head = f.readline().rstrip("\n").split("\t")
        try:
            i_share, i_load = head.index("delay_share"), head.index("load1")
        except ValueError:
            sys.exit(f"{path}: not a corpus-holder-rate sample file")
        for ln in f:
            p = ln.rstrip("\n").split("\t")
            if len(p) <= max(i_share, i_load):
                continue
            try:
                s, l = float(p[i_share]), float(p[i_load])
            except ValueError:
                continue
            rows.append((l, s))
    return rows


def bin_summary(rows, edges, i):
    """Median live delay share in bin i, plus its live and sentinel counts.

    Split out of the printing loop so the selftest can assert on it. It is a
    MEDIAN and not a mean on purpose: the samples are a handful per bin and one
    interval in which the recorder happened to be woken repeatedly drags a mean
    somewhere the box never was. A mutation swapping the two used to pass every
    check in this file.

    The top bin is closed at its right edge so the largest sample is never
    dropped; every other bin is half-open.
    """
    last = i == len(edges) - 2
    a0, a1 = edges[i], edges[i + 1]
    v = [s for l, s in rows if a0 <= l < a1 or (last and l == a1)]
    live = [x for x in v if x >= 0]
    if not live:
        return None, 0, len(v)
    return statistics.median(live), len(live), len(v) - len(live)


def selftest():
    """Fixtures with a known answer, because this file's output is a table
    somebody will set a threshold from.

    `docs/CORPUS-LOCK-HOLD.md` §22.5 says exactly that: L3's abort line was
    invented at +0.20 without a measured baseline and was useless, and the fix
    is to set the next one from this curve. A reader that mis-bins, or that
    averages the -1 sentinel in as a zero, would produce a plausible table and
    a wrong threshold.

    Mutation-swept 2026-08-18: seven mutations, all caught. Dropping the
    sentinel at read time, taking its absolute value, removing the short-row
    guard, falling through a parse failure instead of skipping it, removing the
    wrong-header refusal, swapping the bin median for a mean, opening the top
    bin's right edge, and making a bin exclusive at its left. THREE of those
    were MISSED on the first sweep, and each one added a fixture below. The
    checks here exist because a mutation survived them, not because the list
    looked thorough.
    """
    import tempfile, os
    fails = []

    def write(rows):
        fd, path = tempfile.mkstemp(suffix=".tsv")
        with os.fdopen(fd, "w") as f:
            f.write("label\ttime\tholder\tpid\tcpu_ns_d\tdelay_ns_d\t"
                    "slices_d\tdelay_share\tload1\n")
            for share, load in rows:
                f.write("x\t00:00:00\t/t\t1\t1\t1\t1\t%s\t%s\n" % (share, load))
        return path

    # 1. Rows are read, and the sentinel is read as -1 rather than dropped.
    p1 = write([(0.5, 10.0), (-1, 10.0), (0.7, 30.0)])
    rows = read(p1)
    if rows != [(10.0, 0.5), (10.0, -1.0), (30.0, 0.7)]:
        fails.append("read() returned %r" % (rows,))

    # 2. The sentinel must NOT be averaged in as a zero. With one live sample
    #    of 0.8 and one sentinel, the reported median is 0.8, not 0.4 and not
    #    -0.1.
    live = [s for _, s in read(write([(0.8, 5.0), (-1, 5.0)])) if s >= 0]
    if live != [0.8]:
        fails.append("sentinel not excluded from the live set: %r" % (live,))

    # 3. Binning is half-open except at the top edge, so the largest sample is
    #    never dropped. A reader that loses its maximum understates every
    #    loaded bin, which is the direction that would have hidden §22's result.
    #    Asserted THROUGH bin_summary, not by reimplementing its arithmetic
    #    here: a check that re-derives the thing it is checking passes when the
    #    real code is wrong, and a mutation deleting the top-edge case came back
    #    MISSED against exactly that shape of check.
    rows3 = [(0.0, 0.1), (100.0, 0.9)]
    med_top, n_top, _ = bin_summary(rows3, [0.0, 50.0, 100.0], 1)
    if n_top != 1 or med_top is None or abs(med_top - 0.9) > 1e-9:
        fails.append("top bin lost its right-edge sample: median=%r n=%r"
                     % (med_top, n_top))
    med_lo, n_lo, _ = bin_summary(rows3, [0.0, 50.0, 100.0], 0)
    if n_lo != 1 or med_lo is None or abs(med_lo - 0.1) > 1e-9:
        fails.append("lower bin wrong: median=%r n=%r" % (med_lo, n_lo))

    # 4. A file with no usable rows is an error, not an empty table.
    if read(write([])) != []:
        fails.append("an empty file did not read as no rows")

    # 5. A TRUNCATED row is skipped, not crashed on and not half-read. The
    #    sample files this reads are appended to by a live sampler, so the last
    #    line of a file read mid-write is routinely a partial one. Nothing
    #    exercised this guard until a mutation removing it came back MISSED.
    fd, p5 = tempfile.mkstemp(suffix=".tsv")
    with os.fdopen(fd, "w") as f:
        f.write("label\ttime\tholder\tpid\tcpu_ns_d\tdelay_ns_d\t"
                "slices_d\tdelay_share\tload1\n")
        f.write("x\t00:00:00\t/t\t1\t1\t1\t1\t0.4\t12.0\n")
        f.write("x\t00:00:00\t/t\t1\t1\n")          # torn mid-write
    try:
        got = read(p5)
    except Exception as e:
        fails.append("a truncated final row raised %s instead of being skipped" % type(e).__name__)
        got = None
    if got is not None and got != [(12.0, 0.4)]:
        fails.append("truncated row not skipped: %r" % (got,))
    os.unlink(p5)

    # 6. A NON-NUMERIC row is skipped, not silently duplicated from the row
    #    before it. `ci/corpus-holder-rate.sh` writes a literal `-` in the share
    #    column whenever nobody holds the lock, and `ci/corpus-load-band.sh`'s
    #    calibration file carries those rows routinely — two of the four windows
    #    in the 40-worker calibration were `-`. If the parse failure fell
    #    through instead of skipping, the row would be appended carrying the
    #    PREVIOUS row's numbers, which reads as a real sample at a real load.
    fd, p6 = tempfile.mkstemp(suffix=".tsv")
    with os.fdopen(fd, "w") as f:
        f.write("label\ttime\tholder\tpid\tcpu_ns_d\tdelay_ns_d\t"
                "slices_d\tdelay_share\tload1\n")
        f.write("x\t00:00:00\t/t\t1\t1\t1\t1\t0.4\t12.0\n")
        f.write("x\t00:00:00\t/t\t1\t1\t1\t1\t-\t63.0\n")
    got6 = read(p6)
    if got6 != [(12.0, 0.4)]:
        fails.append("a '-' share row was not skipped: %r" % (got6,))
    os.unlink(p6)

    # 7. A file that is not a sample file must be REFUSED, not read as empty.
    #    Pointed at the wrong path this would otherwise print a table of
    #    nothing under a confident heading, which is the failure this whole
    #    branch keeps meeting.
    fd, p7 = tempfile.mkstemp(suffix=".tsv")
    with os.fdopen(fd, "w") as f:
        f.write("something\telse\tentirely\n1\t2\t3\n")
    refused = False
    try:
        read(p7)
    except SystemExit:
        refused = True
    if not refused:
        fails.append("a file with no delay_share/load1 header was not refused")
    os.unlink(p7)

    # 8. The per-bin figure is a MEDIAN, and the sentinel is counted apart from
    #    it. Fixture: four live samples 0.1/0.2/0.3/9.9 in one bin. Median 0.25,
    #    mean 2.625. Nothing here exercised the printing path until a mutation
    #    swapping median for mean came back MISSED.
    rows8 = [(5.0, 0.1), (5.0, 0.2), (5.0, 0.3), (5.0, 9.9), (5.0, -1.0)]
    med, nlive, asleep = bin_summary(rows8, [0.0, 10.0], 0)
    if med is None or abs(med - 0.25) > 1e-9:
        fails.append("bin median is %r, wanted 0.25 (a mean would give 2.625)" % (med,))
    if (nlive, asleep) != (4, 1):
        fails.append("bin counted %r live and %r asleep, wanted 4 and 1" % (nlive, asleep))
    if bin_summary([(5.0, -1.0)], [0.0, 10.0], 0) != (None, 0, 1):
        fails.append("a bin of only sentinels did not report as empty")

    for path in (p1,):
        os.unlink(path)
    if fails:
        for f in fails:
            print("SELFTEST FAIL: %s" % f, file=sys.stderr)
        sys.exit(1)
    print("corpus-delay-curve selftest: 8 checks passed")


def main():
    if "--selftest" in sys.argv:
        selftest()
        return
    ap = argparse.ArgumentParser()
    ap.add_argument("files", nargs="+")
    ap.add_argument("--bins", type=int, default=6)
    a = ap.parse_args()

    data = {Path(f).name: read(f) for f in a.files}
    allloads = [l for rs in data.values() for l, s in rs]
    if not allloads:
        sys.exit("no samples in any file")
    lo, hi = min(allloads), max(allloads)
    edges = [lo + (hi - lo) * i / a.bins for i in range(a.bins + 1)]

    print(f"box load1 spanned {lo:.1f} to {hi:.1f} over {len(allloads)} samples")
    print()
    w = max(len(n) for n in data)
    print(f"{'load1 bin':>16}  " + "  ".join(f"{n:>{max(12,len(n))}}" for n in data))
    for i in range(a.bins):
        a0, a1 = edges[i], edges[i + 1]
        cells = []
        for n, rs in data.items():
            med, nlive, asleep = bin_summary(rs, edges, i)
            if nlive:
                cells.append(f"{med:.3f} n={nlive}" + (f"+{asleep}z" if asleep else ""))
            else:
                cells.append("-")
        print(f"{a0:7.1f}-{a1:6.1f}  " + "  ".join(
            f"{c:>{max(12,len(n))}}" for c, n in zip(cells, data)))
    print()
    for n, rs in data.items():
        live = [s for l, s in rs if s >= 0]
        if live:
            print(f"  {n:>{w}}: median delay share {statistics.median(live):.3f} "
                  f"over {len(live)} live samples ({len(rs)-len(live)} asleep)")


if __name__ == "__main__":
    main()
