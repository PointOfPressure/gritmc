#!/usr/bin/env bash
# corpus-integrity-108.sh — does a concurrent corpus recording agree with a
# serial one over the WHOLE 108-scenario corpus, not the eight-check subset?
#
#     ./ci/corpus-integrity-108.sh <blocks> <outdir> <tree> <tree> [tree...]
#
# Read `docs/CORPUS-LOCK-HOLD.md` §17 before this file. §17 is the
# pre-registration: the arms, the N each one needs, the homogeneity control and
# the four predictions were committed before any of this ran.
#
# # WHAT §16 LEFT OPEN, WHICH IS THE ONLY REASON THIS EXISTS
#
# §16 measured the integrity floor of a concurrent arm against a serial one and
# got 0/8 in both. Its own two limits were that the ensembles were
# heterogeneous (sixteen trees, sixteen recorder identities, one recording
# each) and that the corpus was **eight scenarios rather than 108**. A longer
# run holds more world state and streams more chunks, so a divergence that only
# appears deep into a corpus could not have shown there. This closes both: the
# scratch trees carry a byte-identical `oracle-runner`, so every recording in
# every arm is from one recorder, and the corpus is the whole 108.
#
# # IT DOES NOT WEAKEN THE LOCK, AND CANNOT
#
# Identical to `ci/corpus-exclusion-experiment.sh`'s three properties, for the
# same reasons, and they are re-asserted here rather than inherited by comment:
#
#   * **a private lock namespace per tree.** `RUSTMC_CORPUS_LOCK_DIR` is the
#     lock's own supported variable. `corpus_lock_acquire` is called normally
#     by the selfcheck and is not stubbed; two private namespaces mean the two
#     runs do not exclude each other, and neither takes a slot in the real
#     queue at `~/.rustmc-corpus-lock`;
#   * **ports outside the derived range.** `pick_port_pair` allocates in
#     26000-26999. This rig starts at 27300 and steps by 100 a tree, so no
#     production tree can derive a pair that collides while it runs;
#   * **scratch trees only**, refused below by asking the real lock rather than
#     by pattern-matching a directory name.
#
# # WHY THE RECORDER IS `ci/t2-gameplay-selfcheck.sh` AND NOT A NEW SCRIPT
#
# The selfcheck already records the whole corpus against a cold-booted vanilla
# N times, on the gate's own substrate (the same four game rules, the same
# `minecraft:flat`, the same pinned seed, the same `spawn-radius=0` that stops
# `fudgeSpawnLocation` moving the join), and stamps every recording with the
# recorder identity `ci/corpus-floor.sh` groups by. A purpose-built recorder
# would have been about 150 lines that must not drift from `ci/t2-gameplay.sh`,
# and any drift would be invisible in the result rather than loud.
#
# It costs a reference recording and no subject recording, which is the half §2
# established is branch-invariant and the half §16 measured, so a run is about
# 22 minutes rather than the 45 a subject recording adds. That is what pays for
# n=18 in the concurrent arm.
#
# **The one difference from the merge gate, named because it is constant across
# arms rather than because it is harmless:** `ci/t2-gameplay.sh` passes
# `--console` to `record` and the selfcheck does not, so the single check of the
# 108 carrying a `[[gamerule]]` block, `780-random-ticks-turn-covered-grass-to-
# dirt`, is driven differently here than in a gate. Both arms drive it the same
# way, so it cannot discriminate between them.
#
# # THE BLOCK STRUCTURE, AND WHY THE ARMS ALTERNATE
#
# This box read load 4 and load 110 within one hour on 2026-08-18, and
# `readiness-falsify` was RED at 11.8 and GREEN at 3.5 on an identical tree.
# Running one arm to completion and then the other would confound the treatment
# with the time of day. So blocks alternate `serial, conc, serial, conc, ...`,
# and the serial block rotates which tree it uses so no tree is only ever
# measured in one arm.
#
# `SELFCHECK_RUNS` is 2 rather than 1 because the selfcheck refuses anything
# below 2 — it compares run 1 against runs 2..N and there is nothing to compare
# a single run to. So a block is about 45 minutes and the alternation is at
# that granularity, which is coarser than
# `ci/corpus-exclusion-experiment.sh`'s three and a half minutes and is the
# price of a corpus that is thirteen times longer.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

BLOCKS="${1:?usage: $0 <blocks> <outdir> <tree> <tree> [tree...]}"
OUT="${2:?outdir}"
shift 2
TREES=("$@")
[ "${#TREES[@]}" -ge 2 ] || { echo "need at least two trees" >&2; exit 2; }

RUNS_PER_INVOCATION="${INTEGRITY108_RUNS:-2}"
PORT_STEP=100
PORT_FIRST=27600

# **`cargo` on `PATH`, or the whole run is a 127 in three seconds.** The
# selfcheck rebuilds `oracle-runner` before it records, and a detached run
# started from a non-login shell has no `~/.cargo/bin`. The failure is silent
# in the shape that matters: every invocation returns immediately, the driver
# reports `0 recording(s)` for each, and a five-hour experiment finishes in
# eleven seconds looking like it ran.
case ":$PATH:" in
    *":$HOME/.cargo/bin:"*) ;;
    *) PATH="$HOME/.cargo/bin:$PATH"; export PATH ;;
esac
command -v cargo >/dev/null || { echo "no cargo on PATH" >&2; exit 4; }

refuse() { echo "CORPUS-INTEGRITY-108 REFUSED — $*" >&2; exit 4; }

for T in "${TREES[@]}"; do
    [ -d "$T" ] || refuse "$T is not a directory"
    [ -x "$T/target/release/oracle-runner" ] || refuse "$T has no built oracle-runner"
    [ -x "$T/ci/t2-gameplay-selfcheck.sh" ] || refuse "$T has no ci/t2-gameplay-selfcheck.sh"
    N=$(find "$T/ci/checks/gameplay" -maxdepth 1 -name '*.toml' 2>/dev/null | wc -l)
    [ "$N" -ge 100 ] || refuse "$T holds $N gameplay checks, which is not the corpus this measures"
    if "$ROOT/ci/corpus-lock.sh" status 2>/dev/null | grep -cF -- "$T" >/dev/null; then
        refuse "$T appears in the real corpus lock's status — it is in the queue, not scratch."
    fi
done
for i in "${!TREES[@]}"; do
    for j in "${!TREES[@]}"; do
        [ "$i" -lt "$j" ] || continue
        [ "${TREES[$i]}" != "${TREES[$j]}" ] || refuse "trees $i and $j are the same directory"
    done
done

# **One recorder, or say so loudly.** §17.3's homogeneity control is an
# assertion about the trees, so it is checked here rather than discovered when
# `ci/corpus-floor.sh` reports three groups of six at the end of five hours.
REC_IDS="$(for T in "${TREES[@]}"; do "$T/target/release/oracle-runner" recorder-id 2>/dev/null || echo unavailable; done | sort -u)"
if [ "$(printf '%s\n' "$REC_IDS" | wc -l)" -ne 1 ]; then
    echo "NOTE: the trees do NOT share one recorder identity:" >&2
    printf '  %s\n' $REC_IDS >&2
    echo "      the ensembles will be heterogeneous, exactly as §16's were," >&2
    echo "      and the homogeneity control §17.3 claims is NOT available." >&2
fi
echo "recorder identity across ${#TREES[@]} trees: $REC_IDS"

mkdir -p "$OUT/rec"

one_invocation() {              # one_invocation <tree-index> <arm> <tag>
    local IDX="$1" ARM="$2" TAG="$3"
    local T="${TREES[$IDX]}"
    local LOG="$OUT/$TAG.log" S E SL EL RC
    local SW="$T/oracle-work/gameplay-selfcheck-integrity108/$TAG"
    export RUSTMC_CORPUS_LOCK_DIR="$OUT/lock-$IDX"
    export RUSTMC_HOLD_PHASE_DIR="$OUT/phases-$IDX"
    export CORPUS_LOCK_ANCHOR_NAME="corpus-int108-$IDX-$$"
    export CORPUS_LOCK_HEARTBEAT=0
    export SELFCHECK_RUNS="$RUNS_PER_INVOCATION"
    export SELFCHECK_PORT_BASE=$((PORT_FIRST + IDX * PORT_STEP))
    export SELFCHECK_WORK="$SW"
    mkdir -p "$RUSTMC_CORPUS_LOCK_DIR" "$RUSTMC_HOLD_PHASE_DIR" "$SW"
    S=$(date +%s); SL=$(cut -d' ' -f1 /proc/loadavg)
    ( cd "$T" && ./ci/t2-gameplay-selfcheck.sh ) > "$LOG" 2>&1
    RC=$?
    E=$(date +%s); EL=$(cut -d' ' -f1 /proc/loadavg)
    # Recordings out, worlds gone. A 108-scenario vanilla world is a few
    # hundred megabytes and there are `blocks * trees * runs` of them; the
    # recordings are what this measures and they are kilobytes.
    local i KEPT=0
    for i in $(seq 1 "$SELFCHECK_RUNS"); do
        if [ -s "$SW/r$i.json" ]; then
            mkdir -p "$OUT/rec/$ARM"
            cp -f "$SW/r$i.json"      "$OUT/rec/$ARM/$TAG-r$i.json"
            cp -f "$SW/r$i.json.prov" "$OUT/rec/$ARM/$TAG-r$i.json.prov" 2>/dev/null || true
            KEPT=$((KEPT + 1))
        fi
        rm -rf "$SW/r$i"
    done
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$TAG" "$ARM" "$IDX" "$T" "$RC" "$KEPT" "$S" "$E" "$SL/$EL" >> "$OUT/runs.tsv"
    echo "   $TAG ($ARM, $(basename "$T")): rc=$RC, $KEPT recording(s), $(( (E - S) / 60 ))m, load $SL -> $EL"
    # **An invocation that produced no recording is an instrument failure, not
    # a data point, and it must not be allowed to thin an arm quietly.** The
    # first smoke run of this driver returned 127 from every invocation because
    # `cargo` was not on `PATH`, finished in eleven seconds, and printed a
    # perfectly ordinary block structure with empty arms underneath it. The
    # tally is written to a file rather than kept in a variable because the
    # concurrent invocations run in subshells and cannot increment the parent's.
    if [ "$KEPT" -eq 0 ]; then
        echo "   !! $TAG PRODUCED NO RECORDING (rc=$RC) — see $LOG"
        echo "$TAG rc=$RC" >> "$OUT/barren.txt"
    fi
    return "$RC"
}

echo "corpus-integrity-108: $BLOCKS block(s), ${#TREES[@]} trees, $RUNS_PER_INVOCATION runs per invocation, into $OUT"
for i in "${!TREES[@]}"; do
    printf '  tree %s: %s  (selfcheck port base %s)\n' "$i" "${TREES[$i]}" "$((PORT_FIRST + i * PORT_STEP))"
done

for b in $(seq 1 "$BLOCKS"); do
    SERIAL_TREE=$(( (b - 1) % ${#TREES[@]} ))
    echo "== block $b serial (tree $SERIAL_TREE)  $(date +%T)  load $(cut -d' ' -f1 /proc/loadavg)"
    one_invocation "$SERIAL_TREE" serial "b${b}s"
    # Fail fast on the very first invocation only. After that a barren block is
    # recorded and the experiment continues, because the recordings already
    # taken are the expensive part and discarding them to punish a later
    # failure helps nobody.
    if [ "$b" = 1 ] && [ -s "$OUT/barren.txt" ]; then
        echo "REFUSING to continue: the first invocation produced no recording." >&2
        echo "Nothing here is measuring anything yet, so stopping costs one block." >&2
        exit 5
    fi
    echo "== block $b conc${#TREES[@]}            $(date +%T)  load $(cut -d' ' -f1 /proc/loadavg)"
    PIDS=()
    for i in "${!TREES[@]}"; do
        one_invocation "$i" "conc${#TREES[@]}" "b${b}c$i" &
        PIDS[$i]=$!
    done
    for i in "${!TREES[@]}"; do wait "${PIDS[$i]}"; done
done
echo "INTEGRITY-108-DONE $(date +%T)"
if [ -s "$OUT/barren.txt" ]; then
    echo
    echo "$(wc -l < "$OUT/barren.txt") invocation(s) produced no recording:"
    sed 's/^/  /' "$OUT/barren.txt"
    echo "The arms below are thinner than the design asked for. Say so when quoting them."
    exit 6
fi
