#!/usr/bin/env bash
# bench-attrib-through.sh — the four-arm RUSTMC_THROUGH test that decides
# whether the unattributed ~14% (docs/BENCH.md, "The unattributed ~14%") is
# inside the surface stage (group A) or outside it (groups B/C/the six
# newly-counted village/ruined-portal commits, all gated >= Through::Decoration).
#
#     ci/bench-attrib-through.sh <base-dir> <base-sha> <tip-dir> <tip-sha> [reps] [radius]
#
# base-dir/tip-dir are directories under $HOME each holding a built
# target/release/worldgen-dump, exactly as ci/bench-pair.sh expects.
#
# # Why this refuses on load, and load average IS the right instrument here
#
# ci/bench-quiet.sh deliberately does NOT use load average, because a
# gameplay corpus run sleeps in settle_ms timers and vanilla boots rather
# than spinning a core, so two overlapping corpus runs look exactly like an
# idle box under that instrument. This script measures something different:
# CPU-bound instructions-retired-at-one-worker over `worldgen-dump full`, and
# a competing CPU-bound neighbour (a cargo build, a merge gate's rustc, or
# this box's TmForever.exe) inflates wall time and cache pressure and can
# starve this process of a core entirely. Load average is exactly the right
# signal for THAT threat, which is why AGENT-BRIEF's brief for this item
# names a load threshold rather than a lock. The corpus lock is a different
# resource and is deliberately not taken here (AGENT-BRIEF: benches sit
# outside it).
#
# # The falsification this script carries
#
# BENCH_ATTRIB_THROUGH_FAKE_LOAD, read only when set, overrides the measured
# load average so the refuse-on-load gate itself can be shown going red and
# green without needing an actually busy or actually quiet box:
#
#     BENCH_ATTRIB_THROUGH_FAKE_LOAD=30 ci/bench-attrib-through.sh --selftest
#         -> refuses, exit 3
#     BENCH_ATTRIB_THROUGH_FAKE_LOAD=0.5 ci/bench-attrib-through.sh --selftest
#         -> proceeds past the gate (then exits 0 without running any arm)
#
# `--selftest` stops right after the gate decision and runs nothing else, so
# the proof costs milliseconds and needs no built binaries.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOGDIR=/home/sayer/grit/logs
mkdir -p "$LOGDIR"

LOAD_CEILING="${BENCH_ATTRIB_THROUGH_LOAD_CEILING:-4.0}"

current_load() {
    if [ -n "${BENCH_ATTRIB_THROUGH_FAKE_LOAD:-}" ]; then
        echo "$BENCH_ATTRIB_THROUGH_FAKE_LOAD"
        return
    fi
    awk '{print $1}' /proc/loadavg
}

gate_or_refuse() {
    local load
    load="$(current_load)"
    echo "bench-attrib-through: 1-min load = $load, ceiling = $LOAD_CEILING"
    # awk comparison: exit 0 (true) when load <= ceiling
    if awk -v l="$load" -v c="$LOAD_CEILING" 'BEGIN{exit !(l+0 <= c+0)}'; then
        echo "bench-attrib-through: box quiet enough, proceeding"
        return 0
    fi
    echo "bench-attrib-through: REFUSED — load $load exceeds ceiling $LOAD_CEILING"
    return 3
}

if [ "${1:-}" = "--selftest" ]; then
    gate_or_refuse
    rc=$?
    echo "bench-attrib-through: --selftest gate rc=$rc (this is the whole point of --selftest: it never runs an arm)"
    exit "$rc"
fi

usage() { sed -n '2,10p' "$0"; exit 2; }
[ $# -ge 4 ] || usage

BASE_DIR="$1"; BASE_SHA="$2"; TIP_DIR="$3"; TIP_SHA="$4"
REPS="${5:-4}"; R="${6:-7}"

gate_or_refuse || exit 3

for bin in "$HOME/$BASE_DIR/target/release/worldgen-dump" "$HOME/$TIP_DIR/target/release/worldgen-dump"; do
    [ -x "$bin" ] || { echo "FATAL: no binary at $bin"; exit 2; }
done

# The sha arguments were a pure label until this check existed: nothing
# verified a directory actually held the source it claimed to. Each dir must
# carry a .bench-sha written by ci/bench-stamp-sha.sh, which only writes it
# after diffing the tree against `git archive <sha>` — so a stale or
# mislabelled directory refuses here instead of silently producing a wrong
# attribution number.
verify_stamp() {                      # verify_stamp <dir-under-HOME> <claimed-sha>
    local dir="$1" sha="$2" stampfile="$HOME/$1/.bench-sha" stamped
    if [ ! -f "$stampfile" ]; then
        echo "FATAL: $HOME/$dir has no .bench-sha — run: ci/bench-stamp-sha.sh $dir $sha"
        exit 2
    fi
    stamped="$(cat "$stampfile")"
    case "$stamped" in
        "$sha"*) ;;
        *) echo "FATAL: $HOME/$dir is stamped '$stamped', not '$sha' — wrong tree for this arm"; exit 2 ;;
    esac
}
verify_stamp "$BASE_DIR" "$BASE_SHA"
verify_stamp "$TIP_DIR" "$TIP_SHA"

STAMP="$(date +%Y%m%dT%H%M%S)"
SUMMARY="$LOGDIR/bench-attrib-through-$STAMP.summary"
: > "$SUMMARY"
{
    echo "# bench-attrib-through $STAMP"
    echo "# base=$BASE_SHA ($BASE_DIR)  tip=$TIP_SHA ($TIP_DIR)"
    echo "# reps=$REPS radius=$R RUSTMC_WORKERS=1"
    echo "# uptime at start: $(uptime)"
} | tee -a "$SUMMARY"

# Two arms ran to "success" on 2026-08-18 with every rep crashed, because the
# exit status below used to be tee's and nothing counted the rows. An arm is
# now believed only if BOTH hold:
#
#   rc == 0                     ci/bench-pair.sh's own status, taken from
#                               PIPESTATUS[0] — through a pipe to tee, a bare
#                               $? is tee's status and is always 0.
#   rows == reps*centres*2      the ABBA design writes exactly one row per
#                               (rep, centre, arm) and drops the warmup. A
#                               short TSV means some rep died even if the
#                               last one happened to exit 0.
#
# centres is read back out of the TSV header bench-pair.sh wrote, so this
# does not re-derive a constant that lives in the other script.
arm_rows_expected() {                 # arm_rows_expected <tsv>
    local n
    n=$(sed -n "s/^#.*centres='\([^']*\)'.*/\1/p" "$1" | head -1 | wc -w)
    [ "$n" -gt 0 ] || { echo 0; return; }
    echo $(( REPS * n * 2 ))
}
# `grep -c X || echo 0` prints "0\n0" on a file with no match (grep prints 0,
# exits 1, the echo adds a second 0), and the `-lt`/`-eq` it feeds then dies on
# "integer expression expected". awk prints exactly one number either way.
arm_rows_actual() { [ -f "$1" ] || { echo 0; return; }; awk '/^[0-9]/{n++} END{print n+0}' "$1"; }

overall_rc=0
clean_arms=""
dirty_arms=""
# **SURFACE FIRST, and the order is the decision rather than a preference.**
#
# Group A — the only candidate paid on every chunk — is INSIDE the surface
# stage; groups B and C are gated `>= Through::Decoration` and cannot run under
# either arm. So the `surface` arm is the one that attributes the +14% and the
# `noise` arm is its control: it says whether anything moved BELOW the surface
# stage, which is what makes a surface-arm difference attributable rather than
# a whole-pipeline drift.
#
# The gate is re-checked before every arm, so a window that closes mid-run
# stops the run. Taking `noise` first spent a short window on the arm that
# decides nothing — and every window this item has seen has been short (the
# one on 2026-08-18 was nine minutes). With this order a half-length window
# yields the decisive arm and an honest "control not taken", which is worth
# more than a control with nothing to control.
for through in surface noise; do
    echo
    echo "############ RUSTMC_THROUGH=$through ############" | tee -a "$SUMMARY"
    # re-check the gate before EACH arm — a neighbour can start between arms
    load_before="$(current_load)"
    gate_or_refuse || { overall_rc=3; dirty_arms="$dirty_arms $through(load)"; break; }
    LABEL="through-$through"
    OUT="$LOGDIR/bench-pair-attrib-through-$through-$STAMP.tsv"
    if [ -n "${BENCH_ATTRIB_THROUGH_FAKE_ARM_FAIL:-}" ]; then
        # Falsification hook, read only when set. "1" substitutes /bin/false so
        # the rc!=0 path can be shown going red; any other value is taken as a
        # path to a stand-in runner, which is how the OTHER half of the check
        # (rc=0 but a short TSV — the exact shape of the 2026-08-18 fake pass)
        # is proven without needing a crashing binary.
        case "$BENCH_ATTRIB_THROUGH_FAKE_ARM_FAIL" in
            1) RUNNER=/bin/false ;;
            *) RUNNER="$BENCH_ATTRIB_THROUGH_FAKE_ARM_FAIL" ;;
        esac
    else
        RUNNER="$ROOT/ci/bench-pair.sh"
    fi
    RUSTMC_THROUGH="$through" RUSTMC_WORKERS=1 \
        BENCH_PAIR_LABEL="$LABEL" BENCH_PAIR_OUT="$OUT" \
        "$RUNNER" \
            "base:$BASE_DIR:$BASE_SHA" "tip:$TIP_DIR:$TIP_SHA" \
            "$REPS" "$R" 2>&1 | tee -a "$LOGDIR/bench-attrib-through-$through-$STAMP.log" \
        | tee -a "$SUMMARY.tail" >/dev/null
    arm_rc="${PIPESTATUS[0]}"
    load_after="$(current_load)"
    want=$(arm_rows_expected "$OUT"); got=$(arm_rows_actual "$OUT")
    if [ "$arm_rc" -ne 0 ] || [ "$want" -eq 0 ] || [ "$got" -ne "$want" ]; then
        {
            echo "  through=$through ARM FAILED — rc=$arm_rc rows=$got/$want"
            echo "  through=$through NO FIGURE from this arm. log: $LOGDIR/bench-attrib-through-$through-$STAMP.log"
            echo "  through=$through load before=$load_before after=$load_after"
        } | tee -a "$SUMMARY"
        dirty_arms="$dirty_arms $through(rc=$arm_rc,rows=$got/$want)"
        overall_rc=1
        continue
    fi
    {
        echo "  through=$through ARM CLEAN — rc=0 rows=$got/$want"
        echo "  through=$through load before=$load_before after=$load_after"
        echo "  through=$through uptime after: $(uptime)"
        echo "  data: $OUT"
    } | tee -a "$SUMMARY"
    clean_arms="$clean_arms $through"
done

{
    echo
    echo "# arms clean:${clean_arms:- none}"
    echo "# arms NOT clean:${dirty_arms:- none}"
    echo "# a figure may be quoted ONLY from an arm listed clean above"
} | tee -a "$SUMMARY"

echo "uptime at end: $(uptime)" | tee -a "$SUMMARY"
echo "summary: $SUMMARY"
exit "$overall_rc"
