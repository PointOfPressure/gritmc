#!/usr/bin/env bash
# corpus-exclusion-experiment.sh — do two concurrent corpus recordings differ
# from two serialised ones, now that the ports are per-tree?
#
#     ./ci/corpus-exclusion-experiment.sh <reps> <outdir> <tree-a> <tree-b>
#
# Read `docs/CORPUS-LOCK-HOLD.md` §11 and §12 before this file. §12 is the
# pre-registration: the arms, the discard rule and the predictions were
# committed before any of this ran, and this script is that design executed.
#
# # IT DOES NOT WEAKEN THE LOCK, AND CANNOT
#
# The one thing this must never do is become the incident it is studying. Three
# properties, none of which relies on remembering:
#
#   * **a private lock namespace per tree.** `RUSTMC_CORPUS_LOCK_DIR` is the
#     lock's own supported variable. Two different private namespaces means the
#     two runs do not exclude each other, *and* neither one takes a slot in the
#     real queue at `~/.rustmc-corpus-lock`. `corpus_lock_acquire` is called
#     normally by both, is not stubbed, and no tracked file is edited. The
#     production lock is untouched for the whole run;
#   * **ports outside the derived range.** `pick_port_pair` allocates in
#     26000-26999. This rig uses 27100/27101 and 27200/27201, so no production
#     tree on this box can derive a pair that collides with the experiment
#     while it runs;
#   * **scratch trees only.** It refuses a tree that is in the real queue or
#     that is a gate directory, below, rather than trusting the caller to have
#     picked a safe one.
#
# # WHY THE ARMS ALTERNATE
#
# This box reads load 4 and load 110 within the same hour, and `readiness-
# falsify` was RED at 11.8 and GREEN at 3.5 on an identical tree. The effect
# being looked for is a slowdown; load also produces a slowdown. Running the
# arms as two blocks would confound the treatment with the time of day, so they
# alternate, and every run records the one-minute load at its start and end.
# `ci/corpus-exclusion-measure.py` reads those back.
#
# # WHAT IT MEASURES, AND WHAT IT REFUSES TO CONCLUDE
#
# Rate, in seconds per scenario of the reference recording. Integrity is a
# separate instrument: the `vanilla.json` of every run is kept beside its log
# so `ci/corpus-floor.sh` can be run over each arm's ensemble. A rate that does
# not collapse says nothing about whether the recordings agree, and the 08-06
# incident was both.
#
# **No number from this is a corpus figure.** It drives
# `ci/t2-gameplay-subset.sh`, whose denominator is whatever was passed on the
# command line, and whose header forbids quoting it as a score. Rates and
# agreements only.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# The timestamper, as its own mode so it can be falsified without a tree, a
# JVM or a lock. See the long note at its call site for why this is not `awk`.
stamp_stream() {
    while IFS= read -r _line; do
        printf '%s.000 %s\n' "$(date +%s)" "$_line"
    done
}
if [ "${1:-}" = "--stamp" ]; then stamp_stream; exit 0; fi

REPS="${1:?usage: $0 <reps> <outdir> <tree> <tree> [tree...]}"
OUT="${2:?outdir}"
shift 2
TREES=("$@")
[ "${#TREES[@]}" -ge 2 ] || { echo "need at least two trees" >&2; exit 2; }

# The eight checks. Fixed, and all of them carry a `settle_ms`, because the
# settle waits are what `ci/corpus-lock.sh`'s header names as the thing that
# collides. A set chosen per-run would make the denominator move between arms.
CHECKS="010-break-block.toml 020-place-block.toml 030-place-log-axis.toml
040-stairs-and-furnace-face-apart.toml 050-torch-falls-with-its-support.toml
070-sand-falls-when-unsupported.toml 090-chest-opens-a-screen.toml
100-redstone-torch-powers-wire.toml"

refuse() { echo "CORPUS-EXCLUSION REFUSED — $*" >&2; exit 4; }

# A tree that is queued for the real lock, or that a gate is using, is not a
# scratch tree. Ask the lock rather than pattern-matching the name.
for T in "${TREES[@]}"; do
    [ -d "$T" ] || refuse "$T is not a directory"
    [ -x "$T/target/release/oracle-runner" ] || refuse "$T has no built oracle-runner"
    if "$ROOT/ci/corpus-lock.sh" status 2>/dev/null | grep -cF -- "$T" >/dev/null; then
        refuse "$T appears in the real corpus lock's status — it is in the queue, not scratch.
         Point this at a tree nobody is gating."
    fi
done
# Every tree distinct, and every derived port pair distinct with it.
for i in "${!TREES[@]}"; do
    for j in "${!TREES[@]}"; do
        [ "$i" -lt "$j" ] || continue
        [ "${TREES[$i]}" != "${TREES[$j]}" ] || refuse "trees $i and $j are the same directory"
    done
done

mkdir -p "$OUT"

one_run() {                      # one_run <tree> <vport> <oport> <slot> <tag>
    local T="$1" VP="$2" OP="$3" SLOT="$4" TAG="$5"
    local LOG="$OUT/$TAG.log" S E SL EL RC
    export RUSTMC_CORPUS_LOCK_DIR="$OUT/lock-$SLOT"
    export RUSTMC_HOLD_PHASE_DIR="$OUT/phases-$SLOT"
    export CORPUS_LOCK_ANCHOR_NAME="corpus-excl-exp-$SLOT-$$"
    export CORPUS_LOCK_HEARTBEAT=0
    export T2_GAMEPLAY_VANILLA_PORT="$VP"
    export T2_GAMEPLAY_RUSTMC_PORT="$OP"
    mkdir -p "$RUSTMC_CORPUS_LOCK_DIR" "$RUSTMC_HOLD_PHASE_DIR"
    S=$(date +%s.%N); SL=$(cut -d' ' -f1 /proc/loadavg)
    # Every line timestamped at the moment it was produced, so the recording
    # window can be read back without the harness having to know it is being
    # measured.
    #
    # **NOT `awk`, AND THIS COST A RUN TO FIND.** `stdbuf -oL awk` sets awk's
    # OUTPUT line-buffered and does nothing to its INPUT: mawk reads stdin in
    # blocks, so a slow writer's lines are all processed in one burst and
    # `systime()` stamps every one of them with the time of the burst. Measured
    # against a writer that echoes three lines two seconds apart:
    #
    #     stdbuf -oL awk       -> 278, 278, 278     every line, one timestamp
    #     the read loop below  -> 278, 280, 282
    #
    # A corpus run emits a line every few seconds for forty minutes, so this
    # would have stamped every line in every run with one value, computed a
    # recording window of about zero, and produced a confident ratio out of
    # nothing. `read` is unbuffered by construction and cannot regress this way;
    # the logs are hundreds of lines, so the shell loop costs nothing that
    # matters. `ci/corpus-exclusion-measure-falsify.sh` case 9 is the guard.
    ( cd "$T" && stdbuf -oL -eL ./ci/t2-gameplay-subset.sh $CHECKS 2>&1 ) \
        | stamp_stream > "$LOG"
    RC=${PIPESTATUS[0]}
    E=$(date +%s.%N); EL=$(cut -d' ' -f1 /proc/loadavg)
    cp -f "$T/oracle-work/gameplay/subset/vanilla.json" "$OUT/$TAG.vanilla.json" 2>/dev/null || true
    printf '%s\t%s\t%s\t%.3f\t%.3f\t%s\t%s\n' "$TAG" "$SLOT" "$RC" "$S" "$E" "$SL" "$EL" \
        >> "$OUT/runs.tsv"
    return "$RC"
}

# Slot names stay letters, so a log filename is still readable and the reader's
# `<pair>-<slot>.log` convention is unchanged.
slot_name() { printf "\\$(printf '%03o' $((97 + $1)))"; }

group() {                        # group <arm> <tag> <K>
    # `i` MUST be local. Without it this function's loops assign the caller's
    # loop variable, and the caller is the rep counter: the first `group` call
    # of rep 1 left `i=2`, so rep 1's concurrent arms were labelled and TAGGED
    # as rep 2's and then overwritten by the real rep 2. The run looked fine
    # and the arms were silently sharing filenames. Caught by reading the
    # progress log, not by any assertion, which is why case 10 now exists.
    local ARM="$1" PT="$2" K="${3:-${#TREES[@]}}"
    local i
    local -a RCS=()
    if [ "$ARM" = serial ]; then
        for i in "${!TREES[@]}"; do
            one_run "${TREES[$i]}" "$((27100 + i * 100))" "$((27101 + i * 100))" \
                "$(slot_name "$i")" "$PT-$(slot_name "$i")"
            RCS[$i]=$?
        done
    else
        local -a PIDS=()
        for i in $(seq 0 $((K - 1))); do
            one_run "${TREES[$i]}" "$((27100 + i * 100))" "$((27101 + i * 100))" \
                "$(slot_name "$i")" "$PT-$(slot_name "$i")" &
            PIDS[$i]=$!
        done
        for i in $(seq 0 $((K - 1))); do wait "${PIDS[$i]}"; RCS[$i]=$?; done
    fi
    printf '%s\t%s' "$PT" "$ARM" >> "$OUT/pairs.tsv"
    for i in "${!RCS[@]}"; do printf '\t%s' "${RCS[$i]}" >> "$OUT/pairs.tsv"; done
    printf '\n' >> "$OUT/pairs.tsv"
}

echo "corpus-exclusion-experiment: $REPS reps, alternating, into $OUT"
for i in "${!TREES[@]}"; do
    printf '  slot %s: %s  (:%s/:%s)\n' "$(slot_name "$i")" "${TREES[$i]}" \
        "$((27100 + i * 100))" "$((27101 + i * 100))"
done
for i in $(seq 1 "$REPS"); do
    echo "== rep $i serial       $(date +%T)  load $(cut -d' ' -f1 /proc/loadavg)"
    group serial "r${i}s"
    for K in $(seq 2 "${#TREES[@]}"); do
        echo "== rep $i conc-$K       $(date +%T)  load $(cut -d' ' -f1 /proc/loadavg)"
        group "conc$K" "r${i}c$K" "$K"
    done
done
echo "EXPERIMENT-DONE $(date +%T)"
echo
"$ROOT/ci/corpus-exclusion-measure.py" "$OUT"
