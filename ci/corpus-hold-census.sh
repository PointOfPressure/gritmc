#!/usr/bin/env bash
# corpus-hold-census.sh — every hold of the corpus lock, end to end, for runs
# that were never instrumented.
#
# `ci/hold-phases.sh` decomposes a hold from inside the run, which is the right
# instrument and has one limitation that matters right now: it only sees runs
# whose tree carries it. There are sixty trees on this box and the six runs
# queued this morning are all on branches that predate it, so the population
# that would answer "what is a hold, at an N, today" is exactly the population
# the inside instrument cannot see.
#
# This one is outside all of them and needs nothing from any tree. The lock
# writes `$CORPUS_LOCK_DIR/holder` on acquire, carrying the tree, the anchor pid
# and `since=<epoch>`; when the holder changes, the previous one's hold ended.
# Sampling that file is enough to time every hold on the box.
#
# WHAT IT COSTS THE THING IT MEASURES: one `stat` and one `read` of a 181-byte
# file per sample. It never opens `run.lock` or `gate.lock`, so it cannot
# contend with an acquire, and it cannot appear in `status`'s fd-holder list and
# be mistaken for a waiter — which `./ci/corpus-lock.sh status` would, since it
# opens the lock file to probe it. That is the whole reason this reads the
# record directly instead of shelling out to `status`.
#
# WHAT IT CANNOT SEE, stated because a census with an unstated blind spot is
# worse than none: a hold shorter than the sample interval, and the boundary
# between two holds that begin and end between samples. Both are visible as a
# `since=` that jumped by more than one interval without an intervening
# observation, and are counted and reported rather than dropped.
#
#     ./ci/corpus-hold-census.sh <out.tsv> [samples] [interval_s]
#     ./ci/corpus-hold-census.sh --report   <out.tsv>
#     ./ci/corpus-hold-census.sh --residual <out.tsv>   (joins the phase mtimes)
set -uo pipefail
CORPUS_LOCK_DIR="${RUSTMC_CORPUS_LOCK_DIR:-$HOME/.rustmc-corpus-lock}"

# --residual joins this census to `ci/corpus-phase-mtimes.sh`, and it answers
# the question neither of them answers alone: **how much of the hold is not a
# recording.** The census times the hold from the lock's own record; the phase
# mtimes time the two recordings from the harness's own output files; the
# difference is everything else the run did while holding the box — the port
# probe, the release build, both server boots, the comparison and the teardown.
#
# The join is on the tree AND on the time: a tree's `report.json` must have been
# written inside the hold being matched, or the two instruments are describing
# different runs. Without that check a tree that ran a corpus yesterday and held
# the lock today would report a nonsense residual with complete confidence.
if [ "${1:-}" = "--residual" ]; then
    python3 - "${2:?holds.tsv}" <<'PY'
import sys, os, json, glob
holds = []
for line in open(sys.argv[1]):
    f = line.rstrip("\n").split("\t")
    if len(f) >= 4 and f[0] == "hold":
        holds.append((f[1], float(f[2]), float(f[3])))
if not holds:
    print("corpus-hold-census: no completed holds to join"); sys.exit(0)

NAMES = ("harness-gamerules.txt", "vanilla.json", "rustmc.json", "report.json")
rows, unmatched = [], []
for tree, a, b in holds:
    g = os.path.join(tree, "oracle-work", "gameplay")
    paths = {k: os.path.join(g, k) for k in NAMES}
    if not all(os.path.exists(v) for v in paths.values()):
        unmatched.append((tree, "no complete phase file set")); continue
    m = {k: os.path.getmtime(v) for k, v in paths.items()}
    if not (a <= m["report.json"] <= b + 60):
        # The recording in that tree is not the one this hold made.
        unmatched.append((tree, "phase files are from another run")); continue
    try:
        n = len(json.load(open(paths["vanilla.json"]))["scenarios"])
    except Exception:
        unmatched.append((tree, "vanilla.json unreadable")); continue
    ref = m["vanilla.json"] - m["harness-gamerules.txt"]
    sub = m["rustmc.json"] - m["vanilla.json"]
    rows.append((os.path.basename(tree), n, b - a, ref, sub))

if not rows:
    print("corpus-hold-census: %d hold(s) timed, none joinable to a recording" % len(holds))
    for t, why in unmatched: print("  %-24s %s" % (os.path.basename(t), why))
    sys.exit(0)

print("%-22s %4s %9s %9s %9s %11s %8s" %
      ("tree", "N", "hold_min", "ref_min", "sub_min", "residual_m", "resid%"))
for t, n, hold, ref, sub in rows:
    resid = hold - ref - sub
    print("%-22s %4d %9.2f %9.2f %9.2f %11.2f %7.1f%%" %
          (t, n, hold / 60, ref / 60, sub / 60, resid / 60, 100 * resid / hold))
r = [(h - rf - sb) / 60 for _t, _n, h, rf, sb in rows]
hh = [h / 60 for _t, _n, h, _rf, _sb in rows]
print("\nn=%d   residual mean %.2f min (%.1f%% of the hold), range %.2f-%.2f" %
      (len(r), sum(r) / len(r), 100 * (sum(r) / len(r)) / (sum(hh) / len(hh)),
       min(r), max(r)))
print("The residual is the port probe, the release build, both server boots, the")
print("comparison and the teardown together. Everything else is recording.")
if unmatched:
    print("\n%d hold(s) not joined:" % len(unmatched))
    for t, why in unmatched: print("  %-24s %s" % (os.path.basename(t), why))
PY
    exit 0
fi

if [ "${1:-}" = "--report" ]; then
    python3 - "${2:?out.tsv}" <<'PY'
import sys, collections
rows = []
for line in open(sys.argv[1]):
    f = line.rstrip("\n").split("\t")
    if len(f) >= 5 and f[0] == "hold":
        rows.append((f[1], float(f[2]), float(f[3]), int(f[4])))
if not rows:
    print("corpus-hold-census: no completed holds observed yet")
    sys.exit(0)
print("corpus-hold-census: %d completed hold(s)\n" % len(rows))
print("%-24s %10s %10s %s" % ("tree", "hold_min", "gaps", "started"))
import datetime
for tree, a, b, g in rows:
    print("%-24s %10.2f %10d %s" % (tree.split("/")[-1], (b - a) / 60, g,
          datetime.datetime.fromtimestamp(a).strftime("%H:%M:%S")))
d = sorted((b - a) / 60 for _t, a, b, _g in rows)
n = len(d)
mean = sum(d) / n
med = d[n // 2] if n % 2 else (d[n // 2 - 1] + d[n // 2]) / 2
print("\nn=%d  mean %.2f min  median %.2f  min %.2f  max %.2f" %
      (n, mean, med, d[0], d[-1]))
unseen = sum(g for *_x, g in rows)
if unseen:
    print("\n!! %d hold boundary/boundaries were inferred from a jumped `since=`"
          " rather than observed. Those holds may bracket a shorter one this"
          " census could not see." % unseen)
PY
    exit 0
fi

OUT="${1:?usage: $0 <out.tsv> [samples] [interval_s]}"
N="${2:-960}"
IV="${3:-15}"
[ -e "$OUT" ] || printf 'kind\ttree\tsince\tended\tgaps\tanchor\n' > "$OUT"

prev_since=""; prev_tree=""; prev_anchor=""; gaps=0; last_seen=""
i=0
while [ "$i" -lt "$N" ]; do
    i=$((i + 1))
    since=""; tree=""; anchor=""
    if [ -r "$CORPUS_LOCK_DIR/holder" ]; then
        since="$(sed -n 's/^since=//p'  "$CORPUS_LOCK_DIR/holder" 2>/dev/null)"
        tree="$(sed -n  's/^tree=//p'   "$CORPUS_LOCK_DIR/holder" 2>/dev/null)"
        anchor="$(sed -n 's/^anchor=//p' "$CORPUS_LOCK_DIR/holder" 2>/dev/null)"
        [ -n "$tree" ] || tree="$(sed -n 's/^dir=//p' "$CORPUS_LOCK_DIR/holder" 2>/dev/null)"
    fi
    if [ -n "$since" ] && [ "$since" != "$prev_since" ]; then
        # The holder changed. The previous one's hold ended somewhere between
        # the last sample that saw it and now; the new holder's own `since` is
        # the tighter bound of the two and is used.
        if [ -n "$prev_since" ]; then
            # A gap: if the new holder started well after we last saw the old
            # one, a whole hold could have fitted in between unobserved.
            if [ -n "$last_seen" ] && [ "$((since - last_seen))" -gt "$((IV * 2))" ]; then
                gaps=$((gaps + 1))
            fi
            printf 'hold\t%s\t%s\t%s\t%s\t%s\n' \
                "$prev_tree" "$prev_since" "$since" "$gaps" "$prev_anchor" >> "$OUT"
            gaps=0
        fi
        prev_since="$since"; prev_tree="$tree"; prev_anchor="$anchor"
    fi
    [ -n "$since" ] && last_seen="$(date +%s)"
    sleep "$IV"
done
