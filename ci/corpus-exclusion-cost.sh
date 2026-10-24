#!/usr/bin/env bash
# corpus-exclusion-cost.sh — if the corpus lock is relaxed to bounded
# concurrency, how many falsifier cases have to be rewritten?
#
#     ./ci/corpus-exclusion-cost.sh          the tally
#     ./ci/corpus-exclusion-cost.sh --list   every case, its tier and why
#
# # WHY A SCRIPT AND NOT A PARAGRAPH
#
# `docs/CORPUS-LOCK-HOLD.md` §15 priced this as "47 cases assert exclusion
# directly" and "cases 2, 3 and 6 invert", which is the right order of magnitude
# and is wrong in both halves: 47 is the suite's count of PASS assertions across
# two control runs, not a count of cases, and case 4 inverts as well as 2, 3 and
# 6. A prose estimate cannot be checked and goes stale the first time somebody
# adds a case.
#
# So the classification lives in a table below, and **this script refuses to
# print a tally if the table and the files disagree** — a case added to either
# falsifier with no entry here is an error, and an entry naming a case that no
# longer exists is an error. The judgement is still mine; its completeness is
# the machine's.
#
# # THE CHANGE BEING PRICED
#
# §15's sketch, which this assumes and does not argue for: the mutex becomes a
# slot array, `run.lock.1 .. run.lock.N`, `flock -n` in order, first free slot
# wins. **`gate.lock` stays exclusive**, so a merge gate still runs alone; the
# hand-back, the childless anchor and kernel-owned liveness are unchanged.
#
# # THE FOUR TIERS, AND WHY THEY ARE NOT ONE NUMBER
#
#   A  the assertion becomes FALSE BY CONSTRUCTION. "no overlap" and "served
#      A B C D" cannot survive a lock that admits N at once. These need a new
#      property written, not an edited string;
#   B  the assertion stands, THE SETUP STOPS CREATING CONTENTION. A case that
#      spawns a holder and three waiters produces no waiters at all under N=4.
#      Mechanical: spawn N more runs;
#   C  the assertion stands, THE TARGET MOVES from one lock file to a slot
#      array. Mechanical, but it touches the part of each case that reads
#      `/proc` and `fuser`, which is where these cases are hardest to get right;
#   D  unchanged. Anchor liveness, SIGKILL recovery, child fd discipline and
#      gate-against-gate exclusion do not depend on how many runs may hold.
#
# # AND THE HALF THAT IS NOT REWORK
#
# Bounded concurrency creates failure modes that **cannot exist today**, so no
# existing injection covers them and no existing case would go red. Those are
# counted separately at the bottom, because they are the part an estimate of
# "rework" silently omits.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

LOCKF="$ROOT/ci/corpus-lock-falsify.sh"
EXCLF="$ROOT/ci/corpus-exclusion-falsify.sh"

# tier | file | case | reason
TABLE='
A|lock|case_exclusion|asserts "4 concurrent runs serialised, no overlap" and a >=12s wall floor for 4x3s holds; under N slots the floor is ceil(4/N)*3s and pairs overlap by design
A|lock|case_burst|asserts "no overlap" for 8 runs + 2 gates and a wall floor of 10 one-second holds; both are exclusion arithmetic
A|lock|case_fifo_service_order|asserts the served order is exactly "A B C D"; with N slots B, C and D enter together and a total order is not defined
A|lock|case_gate_beats_the_queue|asserts the served order is exactly "A G B C" for the same reason
A|lock|case_two_gates_beat_the_queue|asserts an exact order AND that the runs behind the gates did not overlap
A|lock|case_gate_beats_crowd|asserts the order begins "A G"; under N slots a second run can be in with A before the gate
A|lock|case_gate_priority|asserts the gate "went next"; next is ambiguous once runs enter in groups
A|lock|case_observable|asserts status names THE holder and the waiters; status must name up to N holders
B|lock|case_waits_not_fails|spawns a holder and three queued runs; under N>=4 nothing queues and the case tests nothing
B|lock|case_fifo_no_barge|property (a newer run must not take a free place while an older one is queued) is unchanged and RIGHT; the setup needs N runs holding before there is a free place to leave alone
B|lock|case_post_acquire_hand_back|the hand-back is per place rather than per mutex; the branch still exists, the setup must fill N places to reach it
B|lock|case_dead_waiter_no_stall|needs C parked behind B, so it needs N+2 runs rather than 3
B|lock|case_queue_survives_a_death|same shape: the queue it kills a member of has to exist first
C|lock|case_anchor_is_sole_holder|counts fd holders on ONE lock file and requires exactly 1; becomes exactly 1 per slot, over N files
C|lock|case_stale_record_is_pid_not_timeout|reads a single holder record; becomes one record per slot
D|lock|case_gates_serialise|gate.lock stays exclusive, so two gates still must not overlap
D|lock|case_kill9_frees|SIGKILL recovery is per place and does not depend on how many places there are
D|lock|case_kill9_after_waiting|as above
D|lock|case_kill9_with_live_server|as above
D|lock|case_child_cannot_inherit|fd discipline is per place
D|lock|case_dead_anchor_refused|anchor liveness is per place
D|lock|case_anchor_failure_refuses|anchor liveness is per place
D|lock|case_dead_waiter_reaped|the waiter list and its PID liveness are unchanged
D|excl|1|B finishes its release build while A holds; the build is outside the lock either way
A|excl|2|asserts "exactly one recording is live while A holds"; becomes at most N
A|excl|3|asserts B is parked at the acquire and has not picked ports; for N>=2 B proceeds, and the case must use the N+1th run
A|excl|4|asserts the lock status names B as A WAITER; for N>=2 B is a holder. SS15 lists 2, 3 and 6 as the inverting cases and misses this one
D|excl|5|after the holder released, the queued run gets in; still required, with N+1 runs
A|excl|6|asserts the two recordings do not overlap by so much as an instant
D|excl|7|NEGATIVE CONTROL: with the acquire stubbed, both runs record at once. Already runs two recordings concurrently and is unchanged
D|excl|8|one acquire site, positioned after the preflight and before the port pick; a slot array has one acquire site too
D|excl|9|no cargo invocation between the acquire and the port bind
D|excl|10|the acquire..ports-done gap is seconds, measured
'

# What does not exist yet. These are NOT rework: today there is one lock, so a
# lock that admits one too many is not a reachable state and nothing tests for
# it.
NEW='
case|a lock that admits N+1 concurrent runs must be caught|the whole point of "bounded": the property that replaces exclusion needs its own case, or the bound is asserted nowhere
case|all N places must be usable|a slot array that leaks a place degrades to N-1 and then to 1, silently, and today that failure mode is a wedge the suite already catches
injection|admit-one-too-many|the loop bound off by one: flock over 1..N+1
injection|slot-never-released|a released place is never marked free again
injection|ignore-flock-return|two runs land in one place because the acquire does not check flock did anything
injection|random-slot-not-first-free|places are picked at random, so the array does not fill and arrivals queue behind a free place
'

# --- completeness, before any tally ----------------------------------------
fail=0
lock_cases="$(grep -oE '^\s+case_[a-z0-9_]+' "$LOCKF" | tr -d ' ' | sort -u)"
[ -n "$lock_cases" ] || { echo "cost: found no cases in $LOCKF" >&2; exit 4; }
# **The first extractor here missed case 6 and the completeness guard caught
# it**, which is the only reason this comment exists. Case 6 is a python
# heredoc that prints `  ok    6  ...` rather than calling the shell's `ok "6
# ..."`, so a pattern anchored on the quote found nine cases out of ten and
# would have priced the rework off a table that silently disagreed with the
# file. Match the verdict word instead of the quoting, and drop 0, which is not
# a case number and comes from the `[ $? -eq 0 ]` that follows the heredoc.
excl_cases="$(grep -oE '\b(ok|bad|FAIL)\b[^0-9A-Za-z]{0,8}[0-9]+' "$EXCLF" \
    | grep -oE '[0-9]+$' | grep -v '^0$' | sort -un)"
[ -n "$excl_cases" ] || { echo "cost: found no cases in $EXCLF" >&2; exit 4; }

tabled() { printf '%s\n' "$TABLE" | awk -F'|' -v f="$1" 'NF>=4 && $2==f {print $3}' | sort -u; }

for c in $lock_cases; do
    printf '%s\n' "$(tabled lock)" | grep -cx "$c" >/dev/null || {
        echo "UNCLASSIFIED: $LOCKF has $c and this table does not" >&2; fail=1; }
done
for c in $(tabled lock); do
    printf '%s\n' "$lock_cases" | grep -cx "$c" >/dev/null || {
        echo "STALE: this table names $c and $LOCKF does not" >&2; fail=1; }
done
for c in $excl_cases; do
    printf '%s\n' "$(tabled excl)" | grep -cx "$c" >/dev/null || {
        echo "UNCLASSIFIED: $EXCLF has case $c and this table does not" >&2; fail=1; }
done
for c in $(tabled excl); do
    printf '%s\n' "$excl_cases" | grep -cx "$c" >/dev/null || {
        echo "STALE: this table names case $c and $EXCLF does not" >&2; fail=1; }
done
if [ "$fail" = 1 ]; then
    echo >&2
    echo "The tally below would be wrong, so it is not printed. Classify the case." >&2
    exit 5
fi

N_INJ="$(grep -c 'check_injection "' "$LOCKF")"

if [ "${1:-}" = "--list" ]; then
    printf '%s\n' "$TABLE" | awk -F'|' 'NF>=4 {printf "  %s  %-6s %-38s %s\n", $1, $2, $3, $4}'
    echo
    echo "  NEW (no existing coverage, because the failure mode does not exist today):"
    printf '%s\n' "$NEW" | awk -F'|' 'NF>=3 {printf "     %-10s %-44s %s\n", $1, $2, $3}'
    echo
fi

count() { printf '%s\n' "$TABLE" | awk -F'|' -v t="$1" -v f="$2" 'NF>=4 && $1==t && ($2==f || f=="") {n++} END{print n+0}'; }
tot()   { printf '%s\n' "$TABLE" | awk -F'|' -v f="$1" 'NF>=4 && ($2==f || f=="") {n++} END{print n+0}'; }
newc()  { printf '%s\n' "$NEW"   | awk -F'|' -v k="$1" 'NF>=3 && $1==k {n++} END{print n+0}'; }

A=$(count A ""); B=$(count B ""); C=$(count C ""); D=$(count D ""); T=$(tot "")
echo "=================================================================="
echo "COST OF RELAXING EXCLUSIVITY, in falsifier cases"
echo "=================================================================="
printf '  %-56s %3s\n' "existing cases across both falsifiers" "$T"
printf '    %-54s %3s\n' "ci/corpus-lock-falsify.sh property cases" "$(tot lock)"
printf '    %-54s %3s\n' "ci/corpus-exclusion-falsify.sh cases"     "$(tot excl)"
printf '    %-54s %3s\n' "ci/corpus-lock-falsify.sh injections"     "$N_INJ"
echo
printf '  A  %-53s %3s\n' "assertion false by construction, needs a new property" "$A"
printf '  B  %-53s %3s\n' "assertion stands, setup no longer creates contention"  "$B"
printf '  C  %-53s %3s\n' "assertion stands, target moves to a slot array"        "$C"
printf '  D  %-53s %3s\n' "unchanged"                                            "$D"
echo
printf '  %-56s %3s\n' "CASES NEEDING WORK (A+B+C)" "$((A + B + C))"
printf '  %-56s %3s\n' "of which need a property rewritten (A)" "$A"
echo
printf '  %-56s %3s\n' "NEW cases required (no coverage today)"      "$(newc case)"
printf '  %-56s %3s\n' "NEW injections required (no coverage today)" "$(newc injection)"
echo
printf '  %-56s %3s\n' "TOTAL cases to write or rewrite" "$((A + B + C + $(newc case) + $(newc injection)))"
echo
echo "  All $N_INJ existing injections stay: each breaks something that stays"
echo "  broken under a slot array, and the case that catches it is what moves."
echo "=================================================================="
