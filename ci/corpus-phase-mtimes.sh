#!/usr/bin/env bash
# corpus-phase-mtimes.sh — the reference/subject split, harvested from trees
# that have already run a corpus.
#
# This is `docs/CORPUS-LOCK-HOLD.md` §1.1's instrument, which existed as a
# method in prose and as somebody's shell history. It is committed now because
# it is the only one of the four that answers the question **retrospectively**
# and for free: `ci/hold-phases.sh` sees a run only if that run's tree carries
# it, and on 2026-08-18 none of the six queued runs did.
#
# `ci/t2-gameplay.sh` writes a fixed sequence into `oracle-work/gameplay/`, and
# each file's mtime is the instant its phase ended:
#
#     harness-gamerules.txt   the four game rules are delivered; the reference
#                             recording starts here
#     vanilla.json            the reference recording ends
#     rustmc.json             the subject recording ends
#     report.json             the comparison ends
#
# ITS WEAKNESS IS STATED AND IS CHECKED RATHER THAN ARGUED. An mtime is not a
# timestamp of intent — anything that touched one of those four files moves a
# boundary. So a tree is admitted only if its four mtimes are in the order the
# harness must have written them, and the scenario count is read out of the
# recording's own `scenarios` array rather than assumed from the corpus on
# disk, which may have grown since. A tree that fails either is skipped and
# counted, never quietly included with a wrong denominator.
#
# It cannot see the build, the boots, the acquire wait or the teardown, because
# none of those write one of these four files. For those, read
# `./ci/hold-phases.sh report`. The two share no input.
#
#     ./ci/corpus-phase-mtimes.sh [glob-root]
set -uo pipefail
ROOTS="${1:-$HOME/rustmc-*}"

python3 - "$ROOTS" <<'PY'
import glob, json, os, sys, datetime, statistics as st

pat = os.path.join(sys.argv[1], "oracle-work", "gameplay")
NAMES = ("harness-gamerules.txt", "vanilla.json", "rustmc.json", "report.json")
rows, skipped = [], []

for g in sorted(glob.glob(pat)):
    tree = os.path.basename(os.path.dirname(os.path.dirname(g)))
    f = {k: os.path.join(g, k) for k in NAMES}
    missing = [k for k, v in f.items() if not os.path.exists(v)]
    if missing:
        skipped.append((tree, "no " + missing[0]))
        continue
    m = {k: os.path.getmtime(v) for k, v in f.items()}
    seq = [m[k] for k in NAMES]
    if seq != sorted(seq):
        # Something rewrote a file out of order. The boundaries this would
        # produce are not phases, so the tree is refused rather than reported.
        skipped.append((tree, "mtimes out of harness order — a file was touched"))
        continue
    try:
        n = len(json.load(open(f["vanilla.json"]))["scenarios"])
    except Exception as e:
        skipped.append((tree, "vanilla.json unreadable (%s)" % type(e).__name__))
        continue
    ref = m["vanilla.json"] - m["harness-gamerules.txt"]
    sub = m["rustmc.json"] - m["vanilla.json"]
    cmp_ = m["report.json"] - m["rustmc.json"]
    if n < 50:
        skipped.append((tree, "%d scenarios — a subset run, not a corpus" % n))
        continue
    if ref < 60:
        # A served reference (cache hit) writes vanilla.json in 0.1 s. That is
        # a real and interesting hold, but it is not a reference RECORDING and
        # averaging it into a recording rate would understate the rate by
        # however many trees hit the cache.
        skipped.append((tree, "reference took %.1fs — served, not recorded" % ref))
        continue
    rows.append((tree, n, ref, sub, cmp_, m["report.json"]))

if not rows:
    print("corpus-phase-mtimes: no tree carries a complete, ordered corpus recording")
    for t, why in skipped[:10]:
        print("  skipped %-24s %s" % (t, why))
    sys.exit(0)

rows.sort(key=lambda r: -r[5])
print("%-22s %4s %8s %8s %6s %9s %9s %6s  %s" %
      ("tree", "N", "ref_min", "sub_min", "cmp_s", "ref_s/sc", "sub_s/sc", "ref%", "finished"))
for t, n, ref, sub, c, when in rows:
    print("%-22s %4d %8.2f %8.2f %6.1f %9.2f %9.2f %5.1f%%  %s" %
          (t, n, ref / 60, sub / 60, c, ref / n, sub / n, 100 * ref / (ref + sub),
           datetime.datetime.fromtimestamp(when).strftime("%m-%d %H:%M")))

print("\nn=%d tree(s)" % len(rows))
for lbl, i in (("reference s/scenario", 2), ("subject s/scenario", 3)):
    v = [r[i] / r[1] for r in rows]
    print("%-24s mean %.2f  median %.2f  min %.2f  max %.2f"
          % (lbl, st.mean(v), st.median(v), min(v), max(v)))
sh = [100 * r[2] / (r[2] + r[3]) for r in rows]
print("%-24s mean %.1f%%  min %.1f%%  max %.1f%%" % ("reference share", st.mean(sh), min(sh), max(sh)))
rec = [(r[2] + r[3]) / 60 for r in rows]
print("%-24s mean %.2f min  (this is record work only — no build, no boots)"
      % ("in-lock record work", st.mean(rec)))
if skipped:
    print("\n%d tree(s) skipped:" % len(skipped))
    for t, why in skipped[:12]:
        print("  %-22s %s" % (t, why))
PY
