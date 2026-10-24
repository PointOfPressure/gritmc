#!/usr/bin/env bash
# corpus-hold-sample.sh — passive sampler for the corpus lock's INTERNAL split.
#
# `ci/lanes` measured how long a merge gate holds the box-wide corpus lock
# (38.4 min of a 53.9 min gate, n=6). That figure is a total. It cannot say
# which part of the hold depends on the candidate branch, and that is the only
# question worth asking about it: a vanilla reference recording is a function of
# the pinned jar and the scenario set, not of our server's source, so if it is
# the bulk of the hold then every gate of every branch is re-recording an
# identical artefact under an exclusive lock.
#
# This samples the phase markers `ci/t2-gameplay.sh` already prints, once every
# $INTERVAL seconds, and writes the wall-clock time each one FIRST appeared.
# From that the hold decomposes into:
#
#     ports:                 lock acquired
#     == booting vanilla     reference server boot begins
#     vanilla: N/N recorded  reference recording complete   <- branch-invariant
#     == booting rustmc      subject server boot begins
#     rustmc: N/N recorded   subject recording complete     <- branch-dependent
#     T2-GAMEPLAY GREEN      lock released
#
# IT TAKES NOTHING AND WRITES NOWHERE THE GATE READS. Two reads per sample:
# `./ci/corpus-lock.sh status`, which the lock's own header documents as safe
# from any tree at any time, and a `grep` of a log file the gate appends to.
# `ci/lane-measure.sh` established the pattern; this one adds the phase.
#
# --- SECOND INSTRUMENT AS OF 2026-08-18, AND KEPT DELIBERATELY -------------
#
# `ci/hold-phases.sh` now records the same boundaries from INSIDE the run, at
# every marker, on every run and every gate on the box, and it is the one to
# read first: it sees the acquire wait, the release build and the cache verdict,
# none of which this sampler can see because none of them print a phase line,
# and it does not have to be armed by somebody who guessed right about when a
# gate would reach stage 6. That guess is why this file produced no data on
# 2026-08-16 — it was armed and correct and the chain was failing at T0.
#
# This one is kept, and not because deleting it would be work. It shares no
# input with the inside instrument: it reads `./ci/corpus-lock.sh status` and
# the gate's stdout, where `ci/hold-phases.sh` reads its own `date` calls in the
# gate's process. A decomposition confirmed by two instruments that share an
# input is not confirmed, and this project has withdrawn three conclusions that
# rested on one channel nobody re-measured. Arm it when a number from the inside
# instrument is about to be acted on.
#
#     ./ci/corpus-hold-sample.sh <out.tsv> [samples] [interval_s]
#
# Runs `samples` times and exits, so it is armed with run_in_background and
# never waited on. Default 400 x 15s = 100 minutes, comfortably more than one
# gate.
set -uo pipefail
OUT="${1:-/home/sayer/grit/corpus-hold-sample.tsv}"
N="${2:-400}"
INTERVAL="${3:-15}"
GATEDIR="${GATEDIR:-/home/sayer/rustmc-private/gates}"
LOCKTREE="${LOCKTREE:-/home/sayer/grit/repo}"

[ -s "$OUT" ] || printf 'ts\tgatelog\tlock\tholder_since\tmarker\tdetail\n' > "$OUT"

# Marker -> the fixed string that announces it. Order is the order they occur.
# Matched with grep -F so nothing here is a pattern that can silently stop
# matching; a marker that has gone stale shows up as a phase that never fires,
# which is visible in the output rather than silently absent from it.
MARK_KEYS=(stage6 preflight acquired van_boot van_rec_done rus_boot rus_rec_done compared done)
MARK_STRS=(
    "== stage 6: T2 gameplay"
    "preflight: "
    "ports: "
    "== booting vanilla"
    "(counted, not matched)"
    "== booting rustmc"
    "(counted, not matched)"
    "corpus floor"
    "T2-GAMEPLAY"
)

declare -A SEEN=()
LASTLOG=""

for _ in $(seq 1 "$N"); do
    TS="$(date -Is)"
    # The live gate log: simply the newest in the gate dir. Deliberately NOT
    # filtered by mtime — stages 4 and 5 redirect elsewhere and leave this file
    # untouched for twenty minutes at a time, so an mtime window drops the very
    # gate it is meant to follow. A stale pick is self-announcing instead: every
    # marker fires on the first sample, `done` included, and the run is
    # discarded. A new gate resets the marker set through NEWLOG.
    F="$(find "$GATEDIR" -maxdepth 1 -name 'gate-*.log' -printf '%T@ %p\n' 2>/dev/null \
         | sort -rn | head -1 | cut -d' ' -f2-)"
    ST="$(cd "$LOCKTREE" && ./ci/corpus-lock.sh status 2>&1 | sed -n 's/^  state: *//p' | head -1)"
    SINCE="$(cd "$LOCKTREE" && ./ci/corpus-lock.sh status 2>&1 | sed -n 's/.*since=//p' | head -1)"
    [ -n "$ST" ] || ST="?"

    if [ -n "$F" ] && [ "$F" != "$LASTLOG" ]; then
        SEEN=(); LASTLOG="$F"
        printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$TS" "$(basename "$F")" "$ST" "$SINCE" "NEWLOG" "-" >> "$OUT"
    fi

    if [ -n "$F" ]; then
        # Both recordings print the same "scenarios recorded, stamped" line, so
        # count them rather than testing presence: 1 = vanilla done, 2 = rustmc
        # done. Counting is what makes one string serve two phases without a
        # pattern that could match the wrong one.
        NREC="$(grep -cF "scenarios recorded, stamped" "$F" 2>/dev/null || true)"; NREC="${NREC:-0}"
        LAST="$(grep -E '^  recorded [0-9]+/' "$F" 2>/dev/null | tail -1 | awk '{print $2}')"
        for i in "${!MARK_KEYS[@]}"; do
            k="${MARK_KEYS[$i]}"; s="${MARK_STRS[$i]}"
            [ -n "${SEEN[$k]:-}" ] && continue
            hit=""
            case "$k" in
                van_rec_done) [ "$NREC" -ge 1 ] 2>/dev/null && hit=1 ;;
                rus_rec_done) [ "$NREC" -ge 2 ] 2>/dev/null && hit=1 ;;
                *) grep -qF "$s" "$F" 2>/dev/null && hit=1 ;;
            esac
            [ -n "$hit" ] || continue
            SEEN[$k]=1
            printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
                "$TS" "$(basename "$F")" "$ST" "$SINCE" "$k" "rec=${LAST:--}" >> "$OUT"
        done
        # A progress row every sample, so a phase that stalls is visible as a
        # flat counter rather than as a gap between two markers.
        printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$TS" "$(basename "$F")" "$ST" "$SINCE" "tick" "nrec=$NREC last=${LAST:--}" >> "$OUT"
    else
        printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$TS" "-" "$ST" "$SINCE" "tick" "no-live-gate-log" >> "$OUT"
    fi
    sleep "$INTERVAL"
done
