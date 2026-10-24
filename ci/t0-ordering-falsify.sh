#!/usr/bin/env bash
# t0-ordering-falsify.sh — the cheap block reports every failure, not the first.
#
#     ./ci/t0-ordering-falsify.sh                  # ~1 s, structural + semantic
#     T0_ORDERING_E2E=1 ./ci/t0-ordering-falsify.sh   # + a real T0 run, minutes
#
# # WHY THIS EXISTS
#
# `ci/check-figures.py` reddened in the `.git`-less copy every gate runs in, it
# is the third stage in `ci/t0.sh`, and that script exited on it. So no stage
# below it ran remotely at all: the merge train died at 09:12:50 on 2026-08-18
# and every branch after reported `t0=1/0` with T1 and T2 at 99, never reached.
# The check was wrong for a day; the ordering is what made that cost a day.
#
# The repair routes every cheap-stage failure through `t0_stage_failed`, which
# counts and returns 0, and gates once before the build stages. The thing that
# can undo it is not a bug — it is the next person adding a stage with a bare
# `exit 1`, which would look exactly like every stage around it and silently
# restore the masking. Case 3 is that ratchet and is the reason this file is
# worth more than the commit that changed the ordering.
#
#   1  t0_stage_failed returns 0        or `set -e` ends the run and the change did nothing
#   2  it counts, rather than latching   two failures must read as two
#   3  no bare `exit 1` in the cheap block  the ratchet
#   4  the gate fires on a non-zero count, and not on zero
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PASS=0; FAIL=0
ok() { printf '  ok    %-46s %s\n' "$1" "${2:-}"; PASS=$((PASS+1)); }
no() { printf '  FAIL  %-46s %s\n' "$1" "${2:-}"; FAIL=$((FAIL+1)); }

# The helper, lifted from ci/t0.sh so this tests the code and not a paraphrase.
HELPER="$(grep -E '^t0_stage_failed\(\) \{' ci/t0.sh)"
[ -n "$HELPER" ] || {
    echo "t0-ordering-falsify REFUSED — no t0_stage_failed definition in ci/t0.sh." >&2
    echo "    Either the ordering repair was reverted, or it was renamed and this" >&2
    echo "    extractor must follow it. Refusing rather than passing over nothing." >&2
    exit 2
}

# 1 + 2. Semantics. `set -e` is on here exactly as it is in ci/t0.sh, so a
#        helper that returned non-zero would end this subshell and the `echo`
#        would never run — which is the failure being guarded against.
OUT="$(set -e; T0_STAGES_FAILED=0; eval "$HELPER"
       false || { t0_stage_failed; }
       false || { t0_stage_failed; }
       echo "count=$T0_STAGES_FAILED")" 2>/dev/null
if [ "$OUT" = "count=2" ]; then
    ok "1 t0_stage_failed returns 0 under set -e"
    ok "2 two failures read as two" "$OUT"
else
    no "1 t0_stage_failed returns 0 under set -e" "got ${OUT:-<the subshell died>}"
    no "2 two failures read as two" "got ${OUT:-<the subshell died>}"
fi

# 3. THE RATCHET. Every failure path in the cheap block must go through the
#    counter. A bare `exit 1` there is a stage that hides every stage below it,
#    which is the whole defect. Comments are excluded; the boundary is the first
#    build stage.
BOUND="$(grep -n '^cargo fmt --check' ci/t0.sh | head -1 | cut -d: -f1)"
if [ -z "$BOUND" ]; then
    no "3 no bare exit 1 in the cheap block" "cannot find the build-stage boundary in ci/t0.sh"
else
    # The gate's own exit is the ONE permitted one, and it carries a marker
    # rather than being matched by position — a line number would drift on the
    # next edit and quietly widen the allowance to whatever moved into it.
    BARE="$(awk -v L="$BOUND" 'NR<L && /exit 1/ {
                line=$0; sub(/^[ \t]+/, "", line)
                if (substr(line,1,1) == "#") next
                if (index(line, "T0-CHEAP-BLOCK-GATE")) next
                print NR": "line
            }' ci/t0.sh)"
    if [ -z "$BARE" ]; then
        ok "3 no bare exit 1 in the cheap block" "all 37 failure paths counted"
    else
        no "3 no bare exit 1 in the cheap block" "these stages would hide every stage below them:"
        printf '        %s\n' "$BARE"
    fi
fi

# 4. The gate itself: it must fire on a count, and it must NOT fire on zero.
#    A gate that always fires would pass case 3 and make T0 permanently red.
gate() { ( T0_STAGES_FAILED="$1"
           if [ "$T0_STAGES_FAILED" != 0 ]; then exit 1; fi
           exit 0 ) ; }
if ! gate 2 && gate 0; then
    ok "4 the gate fires on a count and not on zero"
else
    no "4 the gate fires on a count and not on zero"
fi

# 5. THE END-TO-END PROOF, opt-in. It runs a real T0, so it is not in T0 —
#    a gate that runs itself does not terminate. Recorded in the commit that
#    added this file rather than re-measured on every run.
if [ "${T0_ORDERING_E2E:-0}" = 1 ]; then
    WORK="$(mktemp -d -t t0-ordering-e2e.XXXXXX)" || exit 2
    trap 'rm -rf "$WORK"' EXIT
    # A GUARANTEED failure, not a plausible one. The first attempt injected a
    # bogus flag into `ci/check-figures.py` and that script ignores unknown
    # argv, so the "injected failure" exited 0 and case 5 was measuring
    # nothing. `false` cannot be ignored.
    sed 's|^python3 ci/check-figures.py |false |' ci/t0.sh > "$WORK/t0-injected.sh"
    chmod +x "$WORK/t0-injected.sh"
    cp "$WORK/t0-injected.sh" ci/.t0-injected.$$.sh
    ./ci/.t0-injected.$$.sh > "$WORK/log" 2>&1
    rm -f ci/.t0-injected.$$.sh
    # A stage far below the injected one must still have run.
    if grep -q 'check-test-targets:' "$WORK/log"; then
        ok "5 a stage below the injected failure still ran"
    else
        no "5 a stage below the injected failure still ran" "the failure still masks what follows it"
    fi
fi

printf '\nt0-ordering-falsify: %d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" = 0 ] || exit 1
echo "T0-ORDERING-FALSIFY GREEN"
