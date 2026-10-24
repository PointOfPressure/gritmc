#!/usr/bin/env bash
# corpus-integrity-arms.sh — read `ci/corpus-integrity-108.sh`'s output and say
# whether the concurrent arm disagreed with itself more than the serial one did.
#
#     ./ci/corpus-integrity-arms.sh <outdir> [checks-dir]
#
# # THE NULL HYPOTHESIS IS THAT THE RECORDINGS DIFFER
#
# Two independent vanilla recordings of one corpus do not agree, for reasons
# that have nothing to do with concurrency: vanilla's harvest is
# non-deterministic in its own right and `-Dmax.bg.threads=1` does not clamp it
# (refuted 2026-08-18 in the nether, where the clamped spread came out WIDER,
# 34/438/460 against 75/134/151, because `Util.makeExecutor` passes the value as
# `ForkJoinPool` parallelism rather than as a thread cap).
#
# So "the recordings differ" is this experiment's null, not its finding. The
# serial arm measures how much they differ anyway; the question is whether the
# concurrent arm differs MORE, and specifically whether any scenario fails to
# reproduce under concurrency that reproduces serially.
#
# **That last set is the whole result.** It is computed here by name, from
# `ci/corpus-floor.py`'s own per-scenario output rather than from the floor
# totals, because two arms can carry the same floor count over different
# scenarios and a comparison of totals would call that agreement.
#
# # WHAT IT REPORTS THAT `ci/corpus-exclusion-integrity.sh` DOES NOT
#
# The resolving power of each arm, on both axes:
#
#     per-scenario   p = 1 - 0.05^(1/n)          what corpus-floor.py prints
#     corpus-wide    q = 1 - 0.05^(1/(n*S))      the limit that moves with S
#
# §16 was n=16 over S=8 and quoted only the first. This experiment changes S by
# a factor of 13.5, so quoting only the first would hide the entire reason it
# was run. See `docs/CORPUS-LOCK-HOLD.md` §17.1.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# `ci/corpus-floor.sh` builds `oracle-runner` and projects every recording
# through it, so this needs cargo on PATH and it is NOT free: the first run in a
# fresh worktree is a release build. Do not run it while an experiment is in
# flight — a two-minute 24-core build landing in one arm and not the other is
# exactly the confound the arms alternate to avoid.
case ":$PATH:" in
    *":$HOME/.cargo/bin:"*) ;;
    *) PATH="$HOME/.cargo/bin:$PATH"; export PATH ;;
esac
command -v cargo >/dev/null || { echo "no cargo on PATH" >&2; exit 4; }

# `--discriminate <outdir> <arm> <arm> [...]` runs ONLY the cross-arm step, over
# `floor-<arm>/floor.json` files that already exist. The whole result of this
# branch comes out of that step, so it has to be drivable from synthetic inputs:
# a discriminator that can only be exercised by five hours of recording is a
# discriminator nobody has seen go red. `ci/corpus-integrity-arms-falsify.sh` is
# what uses it.
DISCRIMINATE=""
if [ "${1:-}" = "--discriminate" ]; then DISCRIMINATE=1; shift; fi

OUT="${1:?usage: $0 <outdir> [checks-dir]}"
CHECKS="${2:-$ROOT/ci/checks/gameplay}"
if [ -z "$DISCRIMINATE" ]; then
    [ -d "$OUT/rec" ]  || { echo "no $OUT/rec — is that a corpus-integrity-108 outdir?" >&2; exit 2; }
    [ -d "$CHECKS" ]   || { echo "no checks dir at $CHECKS" >&2; exit 2; }
fi
S_TOTAL="$(find "$CHECKS" -maxdepth 1 -name '*.toml' 2>/dev/null | wc -l)"

echo "== integrity by arm, corpus of $S_TOTAL scenarios"
[ -n "$DISCRIMINATE" ] || echo "   recordings: $OUT/rec"
[ -n "$DISCRIMINATE" ] || echo "   checks:     $CHECKS"
echo

if [ -n "$DISCRIMINATE" ]; then
    ARMS="$(find "$OUT" -mindepth 1 -maxdepth 1 -type d -name 'floor-*' -printf '%f\n' | sed 's/^floor-//' | sort)"
    [ -n "$ARMS" ] || { echo "no floor-<arm> directories under $OUT" >&2; exit 2; }
else
    ARMS="$(find "$OUT/rec" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort)"
    [ -n "$ARMS" ] || { echo "no arms under $OUT/rec" >&2; exit 2; }
fi

for ARM in $ARMS; do
    [ -z "$DISCRIMINATE" ] || break
    mapfile -t FILES < <(find "$OUT/rec/$ARM" -name '*.json' ! -name '*.prov' | sort)
    N="${#FILES[@]}"
    echo "-- arm $ARM: $N recording(s)"
    if [ "$N" -lt 2 ]; then
        echo "   fewer than two, nothing to compare. Reported, not skipped."
        echo; continue
    fi
    DISTINCT="$(md5sum "${FILES[@]}" | awk '{print $1}' | sort -u | wc -l)"
    echo "   byte-distinct: $DISTINCT of $N"
    if [ "$DISTINCT" -lt "$N" ]; then
        echo "   !! NOT every recording is byte-distinct. §17.4's prediction 1 says the"
        echo "      floor is then zero by construction and this result is WITHDRAWN,"
        echo "      not reported. Do not quote the floor below."
    fi
    RECORDERS="$(cat "$OUT/rec/$ARM"/*.prov 2>/dev/null | sed -n 's/^recorder=//p' | sort -u | tr '\n' ' ')"
    NREC="$(printf '%s\n' $RECORDERS | grep -c . || true)"
    echo "   recorders:     ${RECORDERS:-none stamped}  (${NREC} distinct)"
    [ "$NREC" = 1 ] && echo "   homogeneous: one recorder, so a disagreement cannot be a decoder difference" \
                    || echo "   HETEROGENEOUS: a disagreement here could be a decoder difference (§16's limit, not closed)"
    python3 -c '
import sys
n, S = int(sys.argv[1]), int(sys.argv[2])
print("   resolving power: per-scenario %.2f%%   corpus-wide %.3f%%" % (
    (1-0.05**(1.0/n))*100, (1-0.05**(1.0/(n*S)))*100))' "$N" "$S_TOTAL"
    "$ROOT/ci/corpus-floor.sh" --checks "$CHECKS" --id "int108-$ARM" \
        --out "$OUT/floor-$ARM" "${FILES[@]}" 2>&1 | sed 's/^/   /'
    echo "   corpus-floor exit: ${PIPESTATUS[0]}"
    echo
done

echo "== the discriminator: scenarios that fail to reproduce in ONE arm only"
python3 - "$OUT" $ARMS <<'PY'
import json, sys, os
out, arms = sys.argv[1], sys.argv[2:]
sets, loaded = {}, {}
for a in arms:
    p = os.path.join(out, "floor-%s" % a, "floor.json")
    if not os.path.exists(p):
        print("   arm %s: no floor.json, nothing to compare" % a); continue
    doc = json.load(open(p))
    loaded[a] = doc
    # `floor_names` is the list the floor count is made of. Read by name so two
    # arms with equal floor counts over DIFFERENT scenarios are not called
    # agreement. An earlier version read a `scenarios` list that
    # `ci/corpus-floor.py` does not write, found nothing, and would have printed
    # a confident empty discriminator under a nonzero floor.
    if "floor_names" not in doc:
        raise SystemExit("floor.json has no floor_names — corpus-floor.py's "
                         "output shape moved and this reader would report an "
                         "empty set rather than fail")
    bad = set(doc["floor_names"])
    sets[a] = bad
    print("   %-8s floor %s/%s, %d scenario(s) with more than one projection"
          % (a, doc.get("floor"), doc.get("scenarios_common"), len(bad)))
if len(sets) >= 2:
    ser = [a for a in sets if a.startswith("serial")]
    con = [a for a in sets if not a.startswith("serial")]
    if ser and con:
        s0 = set().union(*[sets[a] for a in ser])
        c0 = set().union(*[sets[a] for a in con])
        only_c, only_s = sorted(c0 - s0), sorted(s0 - c0)
        print()
        print("   concurrent-only (THE RESULT THAT WOULD OVERTURN §15): %d" % len(only_c))
        for n in only_c: print("      %s" % n)
        print("   serial-only (vanilla's own variance, not evidence about the lock): %d" % len(only_s))
        for n in only_s: print("      %s" % n)
        print("   in both: %d" % len(c0 & s0))
PY
echo
echo "== matched n: the serial arm's floor placed in the concurrent arm's own distribution"
echo "   (see §19: the floor grows with n, so the raw counts above are not comparable)"
SER_DIR=""; SER_N=0
if [ -n "$DISCRIMINATE" ]; then echo "   (--discriminate: matched-n subsampling needs the recordings; skipped)"; fi
for ARM in $ARMS; do
    [ -z "$DISCRIMINATE" ] || break
    case "$ARM" in serial*) SER_DIR="$OUT/floor-$ARM"
        SER_N="$(find "$OUT/rec/$ARM" -name '*.json' ! -name '*.prov' | wc -l)" ;;
    esac
done
if [ -z "$SER_DIR" ] || [ ! -s "$SER_DIR/floor.json" ]; then
    echo "   no serial arm floor to place; skipped, and said so"
else
    for ARM in $ARMS; do
        case "$ARM" in serial*) continue ;; esac
        [ -s "$OUT/floor-$ARM/ensemble.json" ] || continue
        CN="$(find "$OUT/rec/$ARM" -name '*.json' ! -name '*.prov' | wc -l)"
        [ "$CN" -gt "$SER_N" ] || { echo "   arm $ARM is not larger than the serial arm; matched already"; continue; }
        echo "-- arm $ARM subsampled to n=$SER_N"
        "$ROOT/ci/corpus-floor-subsample.py" "$OUT/floor-$ARM/ensemble.json" \
            --n "$SER_N" --draws "${SUBSAMPLE_DRAWS:-200}" \
            --reference "$SER_DIR/floor.json" --out "$OUT/subsample-$ARM" \
            2>&1 | sed 's/^/   /'
        echo
    done
fi

echo
echo "== how to read this"
echo "   A concurrent arm whose non-reproducing set sits inside the serial arm's"
echo "   is a set of runs that did not interfere. A scenario in the"
echo "   concurrent-only list is interference, and is the result that keeps the"
echo "   lock. An empty concurrent-only list bounds interference at the"
echo "   corpus-wide resolving power printed for that arm, and at nothing"
echo "   tighter."
