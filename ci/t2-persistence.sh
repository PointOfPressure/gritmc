#!/usr/bin/env bash
# t2-persistence.sh — the stage that runs the persistence gates and their
# controls, so that "a world vanilla can open" stops being a claim somebody
# has to remember to check by hand.
#
# THE COUNT IS NOT WRITTEN DOWN ANYWHERE IN THIS FILE. `GATES` below is the
# list, the stage derives every figure it prints from it, and
# `ci/checks/persistence-paths.toml` names each gate by basename with
# `ci/persistence-applies.py --check` refusing one this file does not run. A
# number in a comment is a number every reader takes on trust and nobody
# re-checks; it was five before this sentence was written and had been four
# for a day.
#
# ===========================================================================
# WHY THIS FILE EXISTS
# ===========================================================================
#
# The scripts that stand behind the save path were in **no gate at all**.
# `docs/CI.md` said so plainly until this commit rewrote that section —
# "nothing runs them for you", "a gate nobody
# runs is a gate that has not run" — which is the honest version of the
# problem and not a fix for it. Their record before this file:
#
#   ci/t2-vanilla-loads-ours.sh          written 2026-08, never invoked by
#                                        anything automatic in its whole life
#   ci/t2-vanilla-opens-our-world.sh     written 2026-09-05, hand-run twice
#   ci/t2-player-survives-a-restart.sh   written 2026-09-05, hand-run twice
#   ci/anvil-byte-identity-falsify.sh    written 2026-09-05, hand-run once
#   ci/t2-sigterm-saves.sh               written 2026-09-05, into this stage from
#                                        the first commit that carries it
#
# A hand-run gate does not fail; it lapses. The failure mode is not a red
# nobody sees, it is a green nobody takes — and six weeks later the save path
# has moved and the last reading anybody has is from a tree that no longer
# exists.
#
# ===========================================================================
# WHY IT IS A STAGE OF ITS OWN AND NOT PART OF T0
# ===========================================================================
#
# Every `t2-` script here boots a JVM or two rustmc servers, and two of them
# boot the real vanilla server. T0's whole budget is ~191 s (`docs/CI.md`'s
# stage table); one vanilla boot alone is a large fraction of that, and a
# vanilla boot needs the box-wide corpus lock or it races whatever else is
# booting one. Neither belongs in a gate that runs on every agent push.
#
# So this is stage 7 of `ci/handoff.sh`, after the corpus, and it declares
# itself into the corpus lock exactly as stage 6 does.
#
# ===========================================================================
# THE CONTROLS ARE NOT OPTIONAL AND THAT IS THE POINT OF THE FILE
# ===========================================================================
#
# Running the scripts and printing GREEN would be a stage of exactly the kind
# `docs/AGENT-BRIEF.md`'s HARD RULE refuses: a verdict whose green has never
# been shown it can be red. Each gate here therefore runs **twice** — once for
# real, and once in the arm that must fail:
#
#   t2-vanilla-loads-ours        --negative-control  flips one byte inside the
#                                first-poked chunk's compressed stream. Vanilla
#                                then discards that chunk and regenerates it
#                                from the seed, so the sentinel is gone and
#                                arm A must fail.
#   t2-vanilla-opens-our-world   --negative-control  deletes level.dat and
#                                world_gen_settings.dat; vanilla must fall back
#                                to the decoy seed.
#   t2-player-survives-a-restart --negative-control  wipes the world between
#                                the two boots; the player must come back at
#                                spawn.
#   t2-sigterm-saves             --negative-control  sends SIGKILL instead of
#                                SIGTERM. No program can catch it, so the two
#                                arms differ in exactly one respect and every
#                                other thing is held.
#   anvil-byte-identity-falsify  is itself a control suite — it reinserts three
#                                shipped defects and requires a red for each,
#                                with a green control at both ends. It has no
#                                second arm because it IS the second arm.
#
# **A control that fails to fire is a stage failure, and the exit status
# cannot say so.** Every control arm exits 0 when it behaved — it prints
# `CONTROL GREEN` and returns success, because the control *working* is the
# good outcome. So `run_gate` asserts on the sentinel string and not on the
# status: a script that crashed before reaching its control also exits
# non-zero, and a script that reached its control and found the gate did NOT
# go red prints its own `RED` line. Only the sentinel separates the three.
#
# ===========================================================================
# WHAT THIS STAGE DOES NOT PROVE
# ===========================================================================
#
# It does not prove the save path is correct in a dimension no arm looks at.
# `docs/cold-start/persistence.md` names what is refused by name — entities
# beyond the format, `fluid_ticks`, `isLightOn` — and none of those is
# measured here. A green from this stage is exactly the claims in
# `docs/CI.md`'s persistence table, and nothing wider.
#
# Usage:
#   ci/t2-persistence.sh                 the whole stage: every gate and its control
#   ci/t2-persistence.sh --no-lock       do not take the corpus lock (for a
#                                        caller that already holds it)
#   ci/t2-persistence.sh --only <name>   one gate and its control, by script
#                                        basename without the .sh
#
# Exit status: 0 green, 1 a gate or a control failed, 2 the stage could not
# run at all (no cargo, no jar, a missing script).

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"

TAKE_LOCK=1
ONLY=""
while [ $# -gt 0 ]; do
    case "$1" in
        --no-lock) TAKE_LOCK=0 ;;
        --only) ONLY="${2:-}"; shift ;;
        # An unrecognised argument is a refusal, for `ci/handoff.sh`'s reason:
        # the default here runs everything, so a typo that fell through to the
        # default would silently run a different stage from the one asked for.
        *) echo "T2-PERSISTENCE REFUSED — unrecognised argument '$1'" >&2; exit 2 ;;
    esac
    shift
done

command -v cargo >/dev/null 2>&1 || {
    echo "T2-PERSISTENCE REFUSED — no cargo on PATH; over ssh prefix with: \
export PATH=\"\$HOME/.cargo/bin:\$PATH\"" >&2
    exit 2
}

JAR="$ROOT/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
[ -f "$JAR" ] || {
    echo "T2-PERSISTENCE REFUSED — no vanilla jar at $JAR, so the three gates \
that boot one cannot run and a green here would mean nothing" >&2
    exit 2
}

# --- the manifest -----------------------------------------------------------
#
# `script|control-flag|the sentinel its GREEN prints|the sentinel its CONTROL
# prints`. An empty control flag means the script carries no second arm and
# the fourth field is unused. Written as a table rather than as four calls so
# that adding a gate is one line and cannot forget the control column.
GATES=(
  "ci/anvil-byte-identity-falsify.sh||ANVIL-BYTE-IDENTITY-FALSIFY GREEN|"
  "ci/t2-vanilla-loads-ours.sh|--negative-control|T2-VANILLA-LOADS-OURS GREEN|T2-VANILLA-LOADS-OURS CONTROL GREEN"
  "ci/t2-vanilla-opens-our-world.sh|--negative-control|T2-VANILLA-OPENS-OUR-WORLD GREEN|T2-VANILLA-OPENS-OUR-WORLD CONTROL GREEN"
  "ci/t2-player-survives-a-restart.sh|--negative-control|T2-PLAYER-SURVIVES-A-RESTART GREEN|T2-PLAYER-SURVIVES-A-RESTART CONTROL GREEN"
  "ci/t2-sigterm-saves.sh|--negative-control|T2-SIGTERM-SAVES GREEN|T2-SIGTERM-SAVES CONTROL GREEN"
)

WORK="$ROOT/oracle-work/t2-persistence"
rm -rf "$WORK"; mkdir -p "$WORK" || exit 2

FAILS=0
RAN=0
CONTROLS=0
SUMMARY=""

note() { SUMMARY="$SUMMARY$1"$'\n'; }

# Run one script and require its log to carry `$want`. The **sentinel, not the
# exit status** decides: a script that died on a missing fixture exits
# non-zero without printing its GREEN line, and a script that printed its
# GREEN line and then failed to remove a temp directory would exit non-zero
# having proved what it was asked to. Both are checked, and the sentinel is
# the one that is reported.
run_gate() { # run_gate <label> <logfile> <want-sentinel> <script> [args...]
    local label="$1" log="$2" want="$3"; shift 3
    local t0 rc
    t0=$(date +%s)
    "$@" > "$log" 2>&1
    rc=$?
    local secs=$(( $(date +%s) - t0 ))
    if grep -qF "$want" "$log"; then
        if [ "$rc" -ne 0 ]; then
            note "  !! $label printed its sentinel and exited $rc"
            FAILS=$((FAILS + 1))
            return 1
        fi
        note "  ok $label  ${secs}s"
        return 0
    fi
    note "  !! $label  ${secs}s  exit $rc — no '$want' in $log"
    # The failing script's own diagnosis, so the reader does not have to open
    # the log to learn which arm went. These are the four scripts' own RED and
    # REFUSED lines plus vanilla's region-file complaints.
    grep -aE "RED —|REFUSED|^  !!|invalid sector|stream is truncated" "$log" | head -6 \
        | sed 's/^/     /' >> "$WORK/detail.txt"
    FAILS=$((FAILS + 1))
    return 1
}

# --- the lock ---------------------------------------------------------------
#
# Role `run` by default and `gate` when `ci/handoff.sh` sets it, exactly as
# `ci/t2-gameplay.sh` does. It is taken ONCE around all four rather than per
# script: three acquires would put this stage back in the queue twice for no
# gain, and the whole stage is a single unit of "the box is booting JVMs".
if [ "$TAKE_LOCK" = 1 ]; then
    . "$ROOT/ci/corpus-lock.sh"
    corpus_lock_acquire
fi

# The counts are DERIVED from the manifest above rather than written here.
# A literal "4 gates, 3 controls" is a number a reader takes on trust and
# nobody re-checks, and it went stale the first time a gate was added —
# which is the same defect as the "eight" tables that were twelve.
echo "== T2-PERSISTENCE: ${#GATES[@]} gates, logs under $WORK"

for row in "${GATES[@]}"; do
    IFS='|' read -r script control green controlgreen <<< "$row"
    base=$(basename "$script" .sh)
    [ -n "$ONLY" ] && [ "$ONLY" != "$base" ] && continue
    [ -x "$ROOT/$script" ] || {
        echo "T2-PERSISTENCE REFUSED — $script is missing or not executable" >&2
        exit 2
    }
    RAN=$((RAN + 1))
    echo "-- $base"
    run_gate "$base" "$WORK/$base.log" "$green" "$ROOT/$script"
    if [ -n "$control" ]; then
        CONTROLS=$((CONTROLS + 1))
        echo "-- $base $control"
        run_gate "$base $control" "$WORK/$base.control.log" "$controlgreen" \
            "$ROOT/$script" "$control"
    fi
    # The corpus lock's anchor is checked between gates for the reason
    # `ci/t2-gameplay.sh` checks it between phases: if it has died, a second
    # run can start on top of this one and every boot after this point is
    # racing something. Nothing measured then is worth reporting.
    if [ "$TAKE_LOCK" = 1 ] && declare -F corpus_lock_assert_anchored >/dev/null 2>&1; then
        corpus_lock_assert_anchored
    fi
done

[ "$RAN" -gt 0 ] || { echo "T2-PERSISTENCE REFUSED — --only '$ONLY' matched no gate" >&2; exit 2; }

printf '%s' "$SUMMARY"
[ -s "$WORK/detail.txt" ] && cat "$WORK/detail.txt"

if [ "$FAILS" -eq 0 ]; then
    echo "T2-PERSISTENCE GREEN — $RAN gate(s), $CONTROLS control(s), 0 failed"
    exit 0
fi
echo "T2-PERSISTENCE RED — $FAILS of $((RAN + CONTROLS)) run(s) did not behave as stated"
exit 1
