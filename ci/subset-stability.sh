#!/usr/bin/env bash
# subset-stability.sh — is a NEW check reproducible before it is banked?
#
#     ./ci/subset-stability.sh 6 700-a-fence-gate-opens-when-right-clicked.toml
#     ./ci/subset-stability.sh 6 700-*.toml 710-*.toml      # from the checks dir
#
# Records the named checks against N independently cold-booted vanillas and
# runs `ci/corpus-floor.py` over the recordings, all pairs. The answer is the
# same kind of answer the corpus floor is: which of these checks two runs of
# the *reference* answer differently.
#
# # Why this exists rather than "run the subset twice and eyeball it"
#
# `ci/checks/gameplay/README.md` requires a new check to read stably across
# independent vanilla runs or be declared unstable with its rate. Until now the
# only instrument that could say so was `ci/t2-gameplay-selfcheck.sh`, which
# records the **whole corpus** N times — about five hours of vanilla boots.
# Nobody spends five hours to validate one check, so in practice the rule was
# satisfied by argument, and this project has already paid for that three
# times: `235` was a coin flip found by six consecutive recordings, `360` had
# the same shape and was found by raising the selfcheck from two runs to
# twelve, and `410`'s own repair introduced a third.
#
# A subset of five checks records in about a minute, so N = 6 costs minutes
# instead of hours. **This is a weaker instrument than the selfcheck and the
# difference is not subtle**: the selfcheck controls the recorder and runs the
# whole corpus, so it can see a check destabilised by what ran before it. This
# runs the named checks alone. A check that is stable here and unstable in the
# corpus is exactly the cross-scenario contamination the corpus README warns
# about, and finding one is a reason to reach for the selfcheck, not evidence
# that this script is broken.
#
# # N and what it can hide
#
# `p_hide(N) = 1 - 0.05**(1/N)`, the largest minority probability that still
# shows N identical runs more often than alpha — the arithmetic lives in
# `ci/corpus-floor.py` and is printed in the output rather than repeated here.
# N = 2 hides a 77.6% flap; N = 6 hides 39.3%; N = 12 hides 22.1%. **Do not
# take N = 2.** The default is 6 because that is what the standing corpus floor
# was measured at, so a new check's rate is quoted against a comparable N.
#
# # It is not a gate and produces no score
#
# Same rule as `ci/t2-gameplay-subset.sh`, which it drives: the denominator is
# whatever you passed on the command line. Nothing here may be quoted as a
# corpus figure. What it may be quoted as is a **stability claim about the
# named checks**, with its N beside it, which is the thing the corpus README
# demands and had no cheap instrument for.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

[ "$#" -ge 2 ] || { echo "usage: $0 <runs> <check.toml> [check.toml ...]" >&2; exit 2; }
RUNS="$1"; shift
case "$RUNS" in ''|*[!0-9]*) echo "runs must be a number, got '$RUNS'" >&2; exit 2;; esac
[ "$RUNS" -ge 2 ] || { echo "runs must be at least 2 — one recording cannot disagree with itself" >&2; exit 2; }

OUT="${OUT:-$ROOT/oracle-work/subset-stability}"
ID="${ID:-subset-$(date -u +%Y%m%dT%H%MZ)}"
SUBSET_VANILLA="$ROOT/oracle-work/gameplay/subset/vanilla.json"
SUBSET_CHECKS="$ROOT/oracle-work/gameplay/subset/checks"

rm -rf "$OUT"; mkdir -p "$OUT"
echo "subset-stability: $ID — $RUNS runs over $# check(s)"

for i in $(seq 1 "$RUNS"); do
    echo "subset-stability: run $i/$RUNS"
    # The subset runner boots its own vanilla every time, which is what makes
    # these independent rather than N reads of one server. It also takes the
    # corpus lock per run and hands it back, so a real merge gate can cut in
    # between our runs instead of waiting out all six.
    #
    # Since 2026-08-12 that hand-back also means **re-joining the run queue at
    # the back each time**, because a re-acquire is a new arrival and gets a new
    # place. That is the right trade and it is worth knowing which way it cuts:
    # each of the six waits is now bounded by whoever was already queued at that
    # moment, where before any of them could be overtaken indefinitely — which
    # is how this very script's branch sat queued for 1h58m on 2026-08-11.
    if ! ./ci/t2-gameplay-subset.sh "$@" > "$OUT/run-$i.log" 2>&1; then
        # A subset run exits non-zero when a check does not match, which is
        # the normal state for a new check against an unfinished server and is
        # NOT a reason to stop: the reference recording is what this script
        # wants and it was still taken. Only a missing recording is fatal.
        echo "subset-stability:   (subset exited non-zero — expected for absent/diverge)"
    fi
    [ -s "$SUBSET_VANILLA" ] || { echo "subset-stability: REFUSED — run $i produced no vanilla recording; see $OUT/run-$i.log" >&2; exit 3; }
    cp "$SUBSET_VANILLA" "$OUT/r$i.json"
    # Each run overwrites the same path, so a stale copy from a previous run
    # would be silently re-counted as an independent one. Moving it out of the
    # way makes that impossible rather than unlikely.
    rm -f "$SUBSET_VANILLA"
done

echo
echo "subset-stability: comparing $RUNS recordings, all pairs"
exec ./ci/corpus-floor.sh --checks "$SUBSET_CHECKS" --id "$ID" \
     --out "$OUT/floor" --no-adjudicate "$OUT"/r[0-9]*.json
