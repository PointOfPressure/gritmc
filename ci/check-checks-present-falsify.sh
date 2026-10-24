#!/usr/bin/env bash
# check-checks-present-falsify.sh — prove the guard on THIS tree, not on stubs.
#
#     ./ci/check-checks-present-falsify.sh
#
# WHY BOTH THIS AND --selftest
#
# `ci/check-checks-present.py --selftest` builds synthetic trees out of stub
# scripts and a stub `ci/t0.sh`. That proves the rules, and it proves them
# against a tree the test itself wrote — which is the shape this repo has been
# burned by twice: `0 of 539 stacks opaque` was guaranteed because the step
# feeding the decoder wrote a literal `0`.
#
# So every case below copies the REAL tree, breaks it the way a real branch
# would, and requires a refusal. The removals are the ones that actually
# happened or were one commit away from happening on 2026-08-17:
#
#   1. the real tree is green — without it the rest proves only that the guard
#      can say no;
#   2. delete ci/check-conflict-markers.py alone — what
#      gameplay-block-placement and two others were carrying, except that they
#      reached that state by rebasing onto an older main rather than by
#      deleting;
#   3. delete it AND its two ci/t0.sh invocations — the case nothing else in
#      the project refuses, because the tree is then self-consistent;
#   4. delete all of that AND its manifest row — must still refuse, on the
#      REQUIRED_FLOOR ratchet, which is the whole reason the floor is a
#      constant in the script rather than a number in the TOML;
#   5. lower REQUIRED_FLOOR to match, i.e. do the removal HONESTLY — must go
#      green. A ratchet nobody can legitimately lower is a ratchet people
#      route around, and this case is what makes case 4 a refusal of silence
#      rather than a refusal of change;
#   6. add a new ci/check-*.py that nothing invokes — the other direction, a
#      check that exists and never runs;
#   7. demote a real invocation to a comment — the tree still contains every
#      script and every name, and the check must still refuse. This is the one
#      that would catch a `#` added to quiet a red.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FAIL=0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

check() {
    local label="$1" cond="$2"
    if [ "$cond" = "0" ]; then echo "ok   $label"; else echo "FAIL $label"; FAIL=1; fi
}

# A copy of the real tree's ci/ — that is everything the guard reads, and
# copying the whole repo would pull in target/ and oracle-work.
fresh() {
    local d="$TMP/$1"
    mkdir -p "$d"
    cp -a "$ROOT/ci" "$d/ci"
    echo "$d"
}

run_guard() { ( cd "$1" && python3 ci/check-checks-present.py 2>&1 ); }

# --- case 1: the real tree, green ------------------------------------------
T=$(fresh clean)
OUT="$(run_guard "$T")"; RC=$?
check "case 1: the real tree is green" "$([ "$RC" -eq 0 ] && echo 0 || echo 1)"
[ "$RC" -eq 0 ] || echo "     ($OUT)"

# --- case 2: a required check deleted --------------------------------------
T=$(fresh gone)
rm -f "$T/ci/check-conflict-markers.py"
OUT="$(run_guard "$T")"; RC=$?
check "case 2: deleting ci/check-conflict-markers.py is refused" \
    "$([ "$RC" -ne 0 ] && echo 0 || echo 1)"
check "case 2: the refusal names the check" \
    "$(printf '%s' "$OUT" | grep -c 'MISSING ci/check-conflict-markers.py' >/dev/null && echo 0 || echo 1)"

# --- case 3: deleted together with its invocations --------------------------
T=$(fresh gone-quietly)
rm -f "$T/ci/check-conflict-markers.py"
grep -v 'check-conflict-markers' "$T/ci/t0.sh" > "$T/ci/t0.sh.new" && mv "$T/ci/t0.sh.new" "$T/ci/t0.sh"
OUT="$(run_guard "$T")"; RC=$?
check "case 3: deleting the check AND its t0 invocation is still refused" \
    "$([ "$RC" -ne 0 ] && echo 0 || echo 1)"
check "case 3: the refusal names the check" \
    "$(printf '%s' "$OUT" | grep -c 'MISSING ci/check-conflict-markers.py' >/dev/null && echo 0 || echo 1)"

# --- case 4: and its manifest row ------------------------------------------
T=$(fresh gone-entirely)
rm -f "$T/ci/check-conflict-markers.py"
grep -v 'check-conflict-markers' "$T/ci/t0.sh" > "$T/ci/t0.sh.new" && mv "$T/ci/t0.sh.new" "$T/ci/t0.sh"
python3 - "$T" <<'PY'
import re, sys
p = sys.argv[1] + "/ci/checks/required-checks.toml"
s = open(p).read()
s = re.sub(r'\[\[required\]\]\nscript = "ci/check-conflict-markers\.py"\n'
           r'wired_in = \[[^\]]*\]\nwhy = "[^"]*"\n', '', s)
open(p, "w").write(s)
PY
OUT="$(run_guard "$T")"; RC=$?
check "case 4: removing the manifest row too is STILL refused (the ratchet)" \
    "$([ "$RC" -ne 0 ] && echo 0 || echo 1)"
check "case 4: and the refusal is the floor, naming the count" \
    "$(printf '%s' "$OUT" | grep -c 'the floor in this script is' >/dev/null && echo 0 || echo 1)"

# --- case 5: the same removal done honestly --------------------------------
#
# Case 4 must refuse SILENCE, not change. If lowering the constant did not
# make it green, the guard would be a wall rather than a ratchet and the next
# agent would delete the guard instead of the row.
T=$(fresh gone-honestly)
rm -f "$T/ci/check-conflict-markers.py"
grep -v 'check-conflict-markers' "$T/ci/t0.sh" > "$T/ci/t0.sh.new" && mv "$T/ci/t0.sh.new" "$T/ci/t0.sh"
python3 - "$T" <<'PY'
import re, sys
d = sys.argv[1]
p = d + "/ci/checks/required-checks.toml"
s = open(p).read()
s = re.sub(r'\[\[required\]\]\nscript = "ci/check-conflict-markers\.py"\n'
           r'wired_in = \[[^\]]*\]\nwhy = "[^"]*"\n', '', s)
open(p, "w").write(s)
g = d + "/ci/check-checks-present.py"
s = open(g).read()
# Read the floor rather than hard-coding it. Written as a literal 8 first,
# and it went stale within the hour when the guard registered itself in its
# own manifest and the floor became 9 — a falsify case that silently stops
# exercising its rule is the thing this whole branch is about.
cur = int(re.search(r'(?m)^REQUIRED_FLOOR = (\d+)$', s).group(1))
s = re.sub(r'(?m)^REQUIRED_FLOOR = \d+$', f'REQUIRED_FLOOR = {cur - 1}', s)
open(g, "w").write(s)
PY
OUT="$(run_guard "$T")"; RC=$?
check "case 5: the same removal WITH the floor lowered is green" \
    "$([ "$RC" -eq 0 ] && echo 0 || echo 1)"
[ "$RC" -eq 0 ] || echo "     ($OUT)"

# --- case 6: a check nothing invokes ---------------------------------------
T=$(fresh orphan)
printf '#!/usr/bin/env python3\n' > "$T/ci/check-nobody-calls-me.py"
OUT="$(run_guard "$T")"; RC=$?
check "case 6: a ci/check-*.py nothing invokes is refused" \
    "$([ "$RC" -ne 0 ] && echo 0 || echo 1)"
check "case 6: the refusal names it" \
    "$(printf '%s' "$OUT" | grep -c 'check-nobody-calls-me.py' >/dev/null && echo 0 || echo 1)"

# --- case 7: an invocation demoted to a comment ----------------------------
T=$(fresh commented)
python3 - "$T" <<'PY'
import sys
p = sys.argv[1] + "/ci/t0.sh"
out = []
for line in open(p):
    if line.startswith("python3 ci/check-conflict-markers.py"):
        line = "# " + line
    out.append(line)
open(p, "w").write("".join(out))
PY
OUT="$(run_guard "$T")"; RC=$?
check "case 7: an invocation demoted to a COMMENT is refused" \
    "$([ "$RC" -ne 0 ] && echo 0 || echo 1)"
check "case 7: the refusal says UNWIRED, not MISSING — the file is still there" \
    "$(printf '%s' "$OUT" | grep -c 'UNWIRED ci/check-conflict-markers.py' >/dev/null && echo 0 || echo 1)"

# --- the rule-level cases, on synthetic trees ------------------------------
( cd "$ROOT" && python3 ci/check-checks-present.py --selftest ) > "$TMP/selftest.log" 2>&1
check "case 8: --selftest's own 10 rule cases pass" \
    "$([ $? -eq 0 ] && echo 0 || echo 1)"
tail -1 "$TMP/selftest.log"

if [ "$FAIL" = 0 ]; then
    echo "CHECKS-PRESENT-FALSIFY GREEN — 13 assertions, 8 cases"
else
    echo "CHECKS-PRESENT-FALSIFY RED"
fi
exit "$FAIL"
