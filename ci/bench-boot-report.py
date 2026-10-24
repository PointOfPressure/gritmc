#!/usr/bin/env python3
"""bench-boot-report.py — aggregate ci/bench-boot.py's raw runs.

    ci/bench-boot-report.py results.jsonl [more.jsonl ...]

Aggregation is deliberately separate from measurement. The raw per-run
records are the evidence; this only reduces them, so a later reading can
re-reduce them differently or check this one. It prints, per group:

    n, median, min, max, and the interquartile range

**The spread is reported, never suppressed.** A median with no spread beside
it is the failure mode this whole file exists to avoid — one number that
cannot be told apart from a lucky run. Where n is small the IQR is quoted
anyway and the n is quoted with it, so the reader can discount it.

The `load` column is the 1-minute load average observed *before* each run,
min-max across the group. A timing taken on a box doing something else is
not wrong, it is just a different measurement, and it has to say so.
"""

import json
import statistics
import sys


def q(vals):
    """(n, median, min, max, iqr) for a list of numbers."""
    v = sorted(x for x in vals if x is not None)
    if not v:
        return (0, None, None, None, None)
    n = len(v)
    med = statistics.median(v)
    if n >= 4:
        lo = statistics.median(v[: n // 2])
        hi = statistics.median(v[(n + 1) // 2:])
        iqr = hi - lo
    else:
        iqr = v[-1] - v[0]
    return (n, med, v[0], v[-1], iqr)


def fmt(x, p=2):
    return "-" if x is None else f"{x:.{p}f}"


def main():
    recs = []
    for path in sys.argv[1:]:
        with open(path) as f:
            for line in f:
                line = line.strip()
                if line:
                    recs.append(json.loads(line))
    # The bundler-extraction boot is a different measurement and is never
    # pooled with the timed ones.
    timed = [r for r in recs if not r.get("bundle_extract")]
    extract = [r for r in recs if r.get("bundle_extract")]

    groups = {}
    for r in timed:
        k = (r["flavour"], r["level"], r.get("tag", ""), bool(r.get("cold")),
             bool(r.get("keep_world")))
        groups.setdefault(k, []).append(r)

    print(f"{'flavour':8} {'level':7} {'tag':14} {'cold':5} {'reload':6} "
          f"{'n':>3} {'t_join med':>10} {'min':>7} {'max':>7} {'iqr':>7} "
          f"{'t_tcp med':>9} {'load':>11}")
    for k in sorted(groups):
        g = groups[k]
        n, med, lo, hi, iqr = q([r.get("t_join_s") for r in g])
        _, tmed, _, _, _ = q([r.get("t_tcp_s") for r in g])
        loads = [r["load_before"][0] for r in g if r.get("load_before")]
        lstr = f"{min(loads):.2f}-{max(loads):.2f}" if loads else "-"
        print(f"{k[0]:8} {k[1]:7} {k[2]:14} {str(k[3]):5} {str(k[4]):6} "
              f"{n:>3} {fmt(med):>10} {fmt(lo):>7} {fmt(hi):>7} {fmt(iqr):>7} "
              f"{fmt(tmed):>9} {lstr:>11}")

    print()
    print(f"{'flavour':8} {'level':7} {'tag':14} {'n':>3} "
          f"{'rss@join MB':>12} {'iqr':>7} {'peak MB':>8} {'idle-end MB':>12} "
          f"{'growth MB':>10} {'chunks':>7}")
    for k in sorted(groups):
        g = groups[k]
        n, med, _, _, iqr = q([r.get("rss_at_join_kb") for r in g])
        _, pk, _, _, _ = q([r.get("rss_peak_to_join_kb") for r in g])
        _, ie, _, _, _ = q([r.get("rss_idle_end_kb") for r in g])
        growth = [r["rss_idle_end_kb"] - r["rss_idle_start_kb"]
                  for r in g
                  if r.get("rss_idle_end_kb") and r.get("rss_idle_start_kb")]
        _, gm, _, _, _ = q(growth)
        chunks = [r.get("probe_chunks") for r in g if r.get("probe_chunks") is not None]
        cstr = f"{statistics.median(chunks):.0f}" if chunks else "-"
        mb = lambda v: None if v is None else v / 1024.0
        print(f"{k[0]:8} {k[1]:7} {k[2]:14} {n:>3} "
              f"{fmt(mb(med), 1):>12} {fmt(mb(iqr), 1):>7} {fmt(mb(pk), 1):>8} "
              f"{fmt(mb(ie), 1):>12} {fmt(mb(gm), 1):>10} {cstr:>7}")

    # Cross-check: each server's own account of its startup, beside ours.
    print()
    print(f"{'flavour':8} {'level':7} {'tag':14} {'n':>3} "
          f"{'self-report s':>13} {'spawn prep ms':>13} {'spawn chunks':>12}")
    for k in sorted(groups):
        g = groups[k]
        n, sr, _, _, _ = q([r.get("self_reported_s") for r in g])
        _, sp, _, _, _ = q([r.get("spawn_prep_ms") for r in g])
        sc = [r.get("spawn_chunks") for r in g if r.get("spawn_chunks")]
        print(f"{k[0]:8} {k[1]:7} {k[2]:14} {n:>3} {fmt(sr):>13} "
              f"{fmt(sp, 0):>13} {(str(sc[0]) if sc else '-'):>12}")

    if extract:
        print()
        for r in extract:
            print(f"bundler-extraction boot ({r['flavour']} {r['level']} "
                  f"{r.get('tag', '')}): t_join={fmt(r.get('t_join_s'))}s "
                  f"— NOT pooled above")

    fails = [r for r in timed if r.get("t_join_s") is None]
    if fails:
        print()
        print(f"!! {len(fails)} of {len(timed)} runs never reached a join:")
        for r in fails[:5]:
            print(f"   {r['flavour']} {r['level']} seq={r['seq']} "
                  f"t_tcp={r.get('t_tcp_s')} out={r.get('join_out', '')[:120]}")


if __name__ == "__main__":
    main()
