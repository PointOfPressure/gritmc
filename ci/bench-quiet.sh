#!/usr/bin/env bash
# bench-quiet.sh — is this box actually quiet enough to measure on?
#
#     ci/bench-quiet.sh            # exit 0 quiet, 1 contended
#
# # This is deliberately NOT a load-average test
#
# The load average is the wrong instrument here and the project has already
# paid for using it. A corpus run spends its wall clock in `settle_ms` sleeps
# and vanilla boots rather than on a core: measured 2026-08-07, two
# overlapping runs plus a merge gate held this box at a one-minute load
# average of 0.06-0.30 while owning it completely. An agent read that low load
# as "nothing is contending" and lost a turn. Two overlapping corpus runs look
# exactly like an idle box.
#
# So the question this script asks is not "is the CPU busy" but "does somebody
# already have the box". Load average is printed as context and is never the
# decider.
#
# # And a free GATE lock is not a quiet box either
#
# They are different locks. `ci/gate-lock.sh` keys per gate-DIRECTORY —
# `box.threadripper_rustmc-gate`, `…-blockage`, `…-twin` are three separate
# keys — so it excludes two gates sharing one directory and nothing else. It
# is an rsync/mtime mutex, not a CPU one. The box-wide mutex is the CORPUS
# lock, and a queued corpus run takes the box the instant a gate frees. A
# waiter armed on the gate lock therefore fires directly into the load it was
# waiting out.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
quiet=0

echo "--- corpus lock (the box-wide one) ---"
if ! "$ROOT/ci/corpus-lock.sh" status 2>&1 | sed 's/^/  /'; then
    echo "  (corpus-lock status unavailable)"
fi

echo "--- gate locks (per gate-dir; informational) ---"
"$ROOT/ci/gate-lock.sh" status 2>&1 | grep -E 'box\.|tree\.|state:' | sed 's/^/  /'

echo "--- competing processes ---"
# Anchored patterns: `pgrep -f 'cargo'` matches the shell carrying the
# pattern, and a stray-process count that includes your own pgrep is not a
# finding. Two agents and the orchestrator each hit this on 2026-08-09.
for pat in '[j]ava' '[c]argo' '[r]ustc' '[o]racle-runner' '[w]orldgen-dump' '[b]ench-servers'; do
    # `pgrep -c` prints "0" AND exits 1 when nothing matches, so the obvious
    # `|| echo 0` appends a SECOND zero and `[ "0\n0" -gt 0 ]` dies with
    # "integer expression expected". The test then errored and the pattern was
    # reported CLEAR — so a genuinely busy pattern would have read clear the
    # same way. Measured 2026-08-09: three of six patterns in one preflight
    # were "clear" verdicts from a broken comparison.
    n=$(pgrep -c -f "$pat" 2>/dev/null)
    case "$n" in
        *[!0-9]*|'')
            # Fail CLOSED. A count we cannot read is not a count of zero.
            echo "  UNREADABLE: $pat gave '$n' — treating as CONTENDED"
            quiet=1
            continue
            ;;
    esac
    if [ "$n" -gt 0 ]; then
        echo "  CONTENDED: $n x $pat"
        pgrep -a -f "$pat" 2>/dev/null | head -3 | sed 's/^/      /'
        quiet=1
    else
        echo "  clear: $pat"
    fi
done

echo "--- context only, never the decider ---"
echo "  loadavg: $(cut -d' ' -f1-3 /proc/loadavg)"
echo "  ci-busy: $([ -e /run/ci-busy ] && echo PRESENT || echo absent)"

if [ "$quiet" -eq 0 ]; then
    echo "QUIET"
else
    echo "CONTENDED"
fi
exit "$quiet"
