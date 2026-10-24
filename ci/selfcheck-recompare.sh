#!/usr/bin/env bash
# selfcheck-recompare.sh — re-measure vanilla's reproducibility against SAVED
# recordings, under today's comparator.
#
#     ./ci/selfcheck-recompare.sh
#     ./ci/selfcheck-recompare.sh --checks /tmp/control-corpus
#     RECORDINGS=~/rustmc/oracle-work/gameplay-selfcheck ./ci/selfcheck-recompare.sh
#
# # WHY THIS EXISTS
#
# `ci/t2-gameplay-selfcheck.sh` cold-boots one vanilla server per run and takes
# **about twelve minutes a run** — two and a half hours at N = 12, over five at
# the N = 26 the recordings on the Threadripper were taken at. Its expensive
# half is recording. Its cheap half is `oracle-runner compare`, and that is the
# half a **projection** change moves.
#
# So a change to what a check looks at — `observe`, `within`, `timing`,
# `collapse_repeats` — can be measured against recordings that already exist,
# at the N they were taken at, in seconds. That is the difference between
# "the reference should now be deterministic under this projection" as an
# argument and as a measurement, and this project does not accept the first
# one.
#
# # AND IT IS NOT THE SAME QUESTION AS `ci/corpus-floor.sh` — 2026-08-09
#
# This one re-runs the *declared adjudication* — run 1 against runs 2..N, the
# set of unstable names against `ci/checks/gameplay-unstable.toml` — under a
# changed projection. That is the right instrument for "did my `observe` change
# stabilise this check", which is what it was written for.
#
# `ci/corpus-floor.sh` reads the same recordings and asks a different question:
# how many **distinct** answers the reference gave over **all** pairs, whether
# each disagreement is vanilla's or this harness's own step attribution, and
# what the resolving power of that N was. It produces the standing floor a
# corpus score is quoted beside. Use this one for a projection experiment and
# that one for a floor.
#
# **It cannot replace the selfcheck**, and the line is worth stating: it says
# nothing about a change to the *recorder* or to the harness's interaction with
# a live server, because both sides of every comparison here were recorded by
# whatever version took them. Anything that changes what goes on the wire needs
# real runs. This is for the compare step and nothing else.
#
# # THE CONTROL, WHICH IS NOT OPTIONAL
#
# Rule 7: when a measurement clears every candidate, suspect the measurement. A
# run of this that reports a check newly stable proves nothing on its own —
# the comparator has other changes in it. Run it twice, once against a copy of
# the corpus with the change backed out:
#
#     cp -r ci/checks/gameplay /tmp/control && sed -i '/^collapse_repeats/d' /tmp/control/*.toml
#     ./ci/selfcheck-recompare.sh --checks /tmp/control     # must reproduce the OLD rates
#     ./ci/selfcheck-recompare.sh                           # the new ones
#
# Same binary, same recordings, one variable. If the control does not
# reproduce the rates already recorded in `ci/checks/gameplay-unstable.toml`,
# the instrument moved and neither number means anything.
#
# # THE RECORDINGS AGE, AND THAT IS HANDLED RATHER THAN IGNORED
#
# The corpus grows; the recordings do not. A scenario the recordings predate is
# missing from both traces and `compare` reports `error` for it, which the
# aggregator would read as a newly unstable check — a made-up instability, from
# an instrument, which is the exact failure this file is for detecting. So the
# check set is intersected with the reference trace's own scenario list and the
# script prints how many it dropped.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

RECORDINGS="${RECORDINGS:-$ROOT/oracle-work/gameplay-selfcheck}"
CHECKS="$ROOT/ci/checks/gameplay"
DECLARED="$ROOT/ci/checks/gameplay-unstable.toml"
OUT="${OUT:-$ROOT/oracle-work/gameplay-selfcheck-recompare}"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --checks)      CHECKS="$2"; shift 2 ;;
        --recordings)  RECORDINGS="$2"; shift 2 ;;
        --declared)    DECLARED="$2"; shift 2 ;;
        --out)         OUT="$2"; shift 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

[ -d "$RECORDINGS" ] || { echo "no recordings at $RECORDINGS" >&2; exit 2; }
# `r[0-9]*.json`, and the character class is not decoration: the same directory
# holds `report-N.json` from whichever aggregation ran last, and a plain `r*`
# glob picks those up as recordings. It did, on the first run of this script —
# 26 recordings became "52" and the first `compare` died on a report file.
mapfile -t RUNS < <(ls "$RECORDINGS"/r[0-9]*.json 2>/dev/null | sort -V)
# Two is the minimum that compares anything at all, and a run of this with one
# recording would otherwise print a confident zero-flap table over no
# comparisons.
[ "${#RUNS[@]}" -ge 2 ] || { echo "need at least 2 recordings in $RECORDINGS, found ${#RUNS[@]}" >&2; exit 2; }

cargo build -q -p oracle-runner --release
BIN="$ROOT/target/release/oracle-runner"

rm -rf "$OUT"
mkdir -p "$OUT/checks"
REF="${RUNS[0]}"

# Only the scenarios the recordings actually contain. See the header.
DROPPED=$(python3 - "$REF" "$CHECKS" "$OUT/checks" <<'PY'
import json, pathlib, shutil, sys, re
ref, checks, dest = sys.argv[1], pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3])
recorded = {s["name"] for s in json.load(open(ref))["scenarios"]}
kept = dropped = 0
for f in sorted(checks.glob("*.toml")):
    m = re.search(r'^name\s*=\s*"([^"]+)"', f.read_text(), re.M)
    if m and m.group(1) in recorded:
        shutil.copy(f, dest / f.name)
        kept += 1
    else:
        dropped += 1
print(dropped)
PY
)
echo "recompare: ${#RUNS[@]} recordings in $RECORDINGS, reference $(basename "$REF")"
echo "recompare: checks from $CHECKS, $DROPPED not present in the recordings"

# A failing comparison must not end the sweep — see the same fix and the same
# reasoning in `ci/t2-gameplay-selfcheck.sh`. This script inherited the bug and
# hit it on the identical pair, which is how it was found: a `vacuous` verdict
# on one check aborted the run at the fourth of twenty-five comparisons and
# printed nothing to say the other twenty-one had not happened.
REPORTS=()
COMPARE_FAILED=0
for r in "${RUNS[@]:1}"; do
    n="$(basename "$r" .json)"
    "$BIN" compare "$OUT/checks" --reference "$REF" --subject "$r" \
        --json "$OUT/report-$n.json" > "$OUT/compare-$n.log" 2>&1 \
        || COMPARE_FAILED=$((COMPARE_FAILED + 1))
    REPORTS+=("$OUT/report-$n.json")
done
if [ "$COMPARE_FAILED" -gt 0 ]; then
    echo "recompare: $COMPARE_FAILED of ${#REPORTS[@]} comparisons reported an" \
         "unacceptable verdict; all of them ran"
fi
echo "recompare: ${#REPORTS[@]} pairwise comparisons"

# Same aggregator the real selfcheck uses, including its power test — so a
# check declared unstable at a rate this N could not have resolved is reported
# as unresolved rather than as stale.
"$ROOT/ci/selfcheck-aggregate.py" "$DECLARED" "${REPORTS[@]}"
