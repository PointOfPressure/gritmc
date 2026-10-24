#!/usr/bin/env bash
# ci/batching-selftests.sh — every piece of red-when-broken evidence for the
# batching area, in one command, in seconds.
#
# A check nobody runs is not a check. Four of the five tools in this area ship
# with a `--selftest` and none of them is wired into any gate, so this is the
# single command that runs them — and the single line that would wire them in.
#
# **NOT ADDED TO `ci/t0.sh` HERE, DELIBERATELY.** `ci/t0.sh` is the file the
# brief for this work names as the one forcing five of six collision groups, and
# `ci-merge-docregions-registry-tomls` is a queued branch already editing that
# area. Two branches editing it is precisely the collision this whole exercise
# is about. Once that branch lands, wiring this in is one line in `ci/t0.sh`:
#
#     ./ci/batching-selftests.sh || fail "batching selftests"
#
# What it runs, and roughly what each costs:
#
#   ci/batch-arith.py   --selftest   8 identity groups, closed form        <1 s
#   ci/gate-rate.py     --selftest   8 known-verdict logs + 4 assertions   <1 s
#   ci/ledger-graft.py  --selftest   16 assertions incl. 4 refusals        <1 s
#   ci/pack-census.py   --selftest   7 assertions on synthetic path sets   <1 s
#   ci/batch-falsify.sh DRYRUN       all 19 injection patches still match  ~2 s
#
# The full `ci/batch-falsify.sh` — 35 cases, 19 injections, ~110 s — is NOT run
# here and must be run by hand after touching `ci/batch.sh`. The dry run is
# included because a patch that has stopped matching is a hole in the sweep
# whether or not there are two minutes for the cases, and it is the check that
# caught ten dead patches and two patches matching the wrong text.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

FAIL=""
run() {
    local name="$1"; shift
    if out="$("$@" 2>&1)"; then
        printf '  OK    %s\n' "$name"
    else
        printf '  FAIL  %s\n' "$name"
        printf '%s\n' "$out" | tail -6 | sed 's/^/          | /'
        FAIL="$FAIL $name"
    fi
}

echo "batching-selftests: the evidence for ci/batch.sh and its tools"
run batch-arith   ./ci/batch-arith.py   --selftest
run gate-rate     ./ci/gate-rate.py     --selftest
run ledger-graft  ./ci/ledger-graft.py  --selftest
run pack-census   ./ci/pack-census.py   --selftest
run falsify-patches env BATCH_FALSIFY_DRYRUN=1 ./ci/batch-falsify.sh

if [ -n "$FAIL" ]; then
    echo "batching-selftests: RED —$FAIL"
    exit 1
fi
echo "batching-selftests: GREEN"
echo "  (the full ci/batch-falsify.sh is 35 cases / 19 injections and ~110 s;"
echo "   run it by hand after touching ci/batch.sh)"
