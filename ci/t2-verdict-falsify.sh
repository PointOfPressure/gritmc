#!/usr/bin/env bash
# t2-verdict-falsify.sh — prove ci/t2-gameplay.sh SAYS something when it fails.
#
#     ./ci/t2-verdict-falsify.sh          # ~5 s, no box, no corpus lock
#
# WHY THIS EXISTS
#
# Until 2026-08-18 `ci/t2-gameplay.sh` had no string for its own failure. It
# printed `T2-GAMEPLAY GREEN` on success and `T2-GAMEPLAY REFUSED` when it
# declined to measure; on a genuine corpus regression the comparison printed
# `  REGRESSED <scenario>` and `set -e` ended the run, saying neither. The gate
# was still correctly RED — `ci/handoff.sh` reads the exit status — but every
# reader of the LOG was blind:
#
#   * `ci/gate-rate.py` reported T2 as 143 GREEN and 0 RED over the project's
#     whole history, in the stage that is 66% of a gate;
#   * `docs/AGENT-BRIEF.md`'s recommended waiter matched
#     `T2-GAMEPLAY (GREEN|REFUSED)` and hung forever through this exact case.
#
# The sentinel is additive: one line on an already-failing exit. So the thing
# that has to be proved is not that it appears — it is that it appears ONLY
# then, because a sentinel that fires on a green run would turn every gate red.
#
# NO CORPUS LOCK IS TAKEN. Both injections kill the script before
# `corpus_lock_acquire` (ci/t2-gameplay.sh line ~288), which is why this is
# safe to run at any time, beside a live gate.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FAIL=0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

check() {
    local label="$1" cond="$2"
    if [ "$cond" = "0" ]; then echo "ok   $label"; else echo "FAIL $label"; FAIL=1; fi
}

# --- part 1: the sentinel function, in isolation ---------------------------
#
# Extracted from the real file rather than retyped, so a change to it cannot
# leave these cases testing a stale copy.
sed -n '/^_t2_sentinel() {/,/^}/p' "$ROOT/ci/t2-gameplay.sh" > "$TMP/sentinel.sh"
check "part 1: the sentinel was found in ci/t2-gameplay.sh" \
    "$([ -s "$TMP/sentinel.sh" ] && echo 0 || echo 1)"
# shellcheck disable=SC1091
. "$TMP/sentinel.sh"

for rc in 0 4; do
    out="$(_t2_sentinel "$rc" 2>&1)"
    check "part 1: exit $rc prints NOTHING (green and refused already say so)" \
        "$([ -z "$out" ] && echo 0 || echo 1)"
done
for rc in 1 2 3 5 99; do
    out="$(_t2_sentinel "$rc" 2>&1)"
    check "part 1: exit $rc prints T2-GAMEPLAY RED" \
        "$(printf '%s' "$out" | grep -c '^T2-GAMEPLAY RED' >/dev/null && echo 0 || echo 1)"
done
out="$(_t2_sentinel 1 2>&1)"
check "part 1: the line names the exit status, so a reader can tell them apart" \
    "$(printf '%s' "$out" | grep -c 'exit 1' >/dev/null && echo 0 || echo 1)"

# --- part 2: the REAL script, two injections -------------------------------
fresh() {
    local d="$TMP/$1"
    mkdir -p "$d"
    git -C "$ROOT" archive HEAD 2>/dev/null | tar -x -C "$d" 2>/dev/null \
        || cp -a "$ROOT"/ci "$d"/ 2>/dev/null
    cp "$ROOT/ci/t2-gameplay.sh" "$d/ci/" 2>/dev/null
    echo "$d"
}

# Injection A — a refusal. The work directory does not belong to the tree, which
# is the first preflight check, and it exits 4 through `refuse`. The sentinel
# must stay quiet: `T2-GAMEPLAY REFUSED` is already a string a reader can match,
# and printing RED beside it would make one failure look like two.
T=$(fresh refuse)
ln -sfn /tmp "$T/oracle-work"
( cd "$T" && timeout 60 ./ci/t2-gameplay.sh ) > "$TMP/refuse.log" 2>&1
RC_A=$?
check "part 2A: a preflight refusal still exits 4" \
    "$([ "$RC_A" = 4 ] && echo 0 || echo 1)"
check "part 2A: it prints T2-GAMEPLAY REFUSED" \
    "$(grep -q 'T2-GAMEPLAY REFUSED' "$TMP/refuse.log" && echo 0 || echo 1)"
check "part 2A: and does NOT also print RED — one failure, one verdict" \
    "$(grep -q 'T2-GAMEPLAY RED' "$TMP/refuse.log" && echo 1 || echo 0)"

# Injection B — a non-refusal death. `ci/reference-cache.sh` is sourced under
# `set -e` after the EXIT trap is armed, so removing it kills the script the way
# a failing comparison does: non-zero, and not through `refuse`. THIS IS THE
# CASE THAT WAS SILENT, and it is the whole reason this file exists.
T=$(fresh die)
rm -f "$T/ci/reference-cache.sh"
( cd "$T" && timeout 60 ./ci/t2-gameplay.sh ) > "$TMP/die.log" 2>&1
RC_B=$?
check "part 2B: a non-refusal death exits non-zero and not 4" \
    "$([ "$RC_B" != 0 ] && [ "$RC_B" != 4 ] && echo 0 || echo 1)"
check "part 2B: AND IT NOW SAYS SO — T2-GAMEPLAY RED is in the log" \
    "$(grep -q 'T2-GAMEPLAY RED' "$TMP/die.log" && echo 0 || echo 1)"
check "part 2B: the old waiter pattern still would not have matched it" \
    "$(grep -qE 'T2-GAMEPLAY (GREEN|REFUSED)' "$TMP/die.log" && echo 1 || echo 0)"
check "part 2B: no GREEN line, so handoff's grep -c pass count is unaffected" \
    "$([ "$(grep -c '^T2-GAMEPLAY GREEN' "$TMP/die.log")" = 0 ] && echo 0 || echo 1)"

if [ "$FAIL" = 0 ]; then
    echo "T2-VERDICT-FALSIFY GREEN — 16 assertions, 2 injections, no corpus lock taken"
else
    echo "T2-VERDICT-FALSIFY RED"
fi
exit "$FAIL"
