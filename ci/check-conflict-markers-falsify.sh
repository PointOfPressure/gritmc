#!/usr/bin/env bash
# check-conflict-markers-falsify.sh — prove the conflict-marker guard by
# using it, on the pattern of ci/merge-appendonly-falsify.sh.
#
#     ./ci/check-conflict-markers-falsify.sh
#
# Everything happens under `mktemp -d`; nothing here touches this repo's
# real tree or its git history. Unlike ci/merge-appendonly-falsify.sh this
# needs no throwaway `git init` — ci/check-conflict-markers.py never asks git
# anything (see that script's header for why: T0 usually runs where `.git`
# does not exist at all), so a plain directory of files is a faithful stand-in
# for the tree it will actually be pointed at. The guard under test is the
# real one at ci/check-conflict-markers.py in THIS checkout, invoked by
# absolute path with `--root <throwaway dir>` — a copy would prove nothing
# about what ci/t0.sh actually runs.
#
# Cases:
#
#   1-4. THE NEGATIVE, once per marker form. A throwaway tracked file gets
#        exactly one of `<<<<<<<`, `|||||||`, `=======`, `>>>>>>>` injected at
#        column 0. The guard must refuse (non-zero exit) and name the file and
#        the line. `|||||||` gets its own case rather than riding along with
#        the others because it is the one a `<<<<<<<`/`>>>>>>>`-only guard
#        would miss — the diff3 middle marker, and the one that actually
#        appeared in the 2026-08-16 corruption this guard exists to catch.
#   5.   THE POSITIVE — a clean tree with ordinary prose passes.
#   6.   THE FILES NAMED IN THE GUARD'S OWN DOCSTRING as legitimately
#        containing marker text — ci/merge-appendonly.py (byte-string
#        literals) and ci/merge-appendonly-falsify.sh (a quoted grep
#        pattern) — copied in verbatim from the real tree and scanned. Both
#        must pass clean, proving the column-0 anchoring does its job on the
#        real files rather than on a hand-written stand-in for them.
#   7.   THE SECOND RULE'S NEGATIVE — a file that is exactly five back-to-back
#        copies of itself, no markers anywhere, which is the shape the
#        2026-08-16 corruption would have had if the markers had been hand-
#        deleted during the botched resolution instead of left in. Proves the
#        whole-file-duplication rule catches what the marker rule structurally
#        cannot see.
#   8.   THE SECOND RULE'S POSITIVE CONTROL — a file that looks superficially
#        similar (same length split in two) but differs by one byte in the
#        second half must stay green, proving the rule is exact-match and not
#        a threshold that could be tuned wrong.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GUARD="$ROOT/ci/check-conflict-markers.py"
FAIL=0

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

check() {
    local label="$1" cond="$2"
    if [ "$cond" = "0" ]; then
        echo "ok   $label"
    else
        echo "FAIL $label"
        FAIL=1
    fi
}

run_guard() {
    # $1 = case dir. Prints combined output, returns the guard's exit code.
    python3 "$GUARD" --root "$1"
}

# --- cases 1-4: one marker form each ----------------------------------------
inject_case() {
    local name="$1" marker_line="$2"
    local dir="$WORK/$name"
    mkdir -p "$dir/docs"
    {
        echo "## 1. Base"
        echo "body text, line 2"
        echo "body text, line 3"
        printf '%s\n' "$marker_line"
        echo "body text after the marker"
    } > "$dir/docs/DETERMINISM.md"

    local out
    out="$(run_guard "$dir" 2>&1)"
    local rc=$?
    check "case $name: guard refuses (non-zero exit)" "$([ "$rc" -ne 0 ] && echo 0 || echo 1)"
    check "case $name: names the file" \
        "$(printf '%s' "$out" | grep -cF 'docs/DETERMINISM.md:4:' >/dev/null && echo 0 || echo 1)"
    check "case $name: names the marker line's content" \
        "$(printf '%s' "$out" | grep -cF "CONFLICT-MARKER" >/dev/null && echo 0 || echo 1)"
}

inject_case "ours"    "<<<<<<< HEAD"
inject_case "base"    "||||||| base"
inject_case "sep"     "======="
inject_case "theirs"  ">>>>>>> feature-branch"

# --- case 5: the positive — a clean tree passes -----------------------------
CLEAN="$WORK/clean"
mkdir -p "$CLEAN/docs"
{
    echo "## 1. Base"
    echo "This paragraph discusses <<<<<<< as prose mid-sentence, which is"
    echo "not at column 0 and must not trip the guard."
} > "$CLEAN/docs/DETERMINISM.md"
CLEAN_OUT="$(run_guard "$CLEAN" 2>&1)"
CLEAN_RC=$?
check "case 5: a clean tree (marker text mid-line) passes" "$([ "$CLEAN_RC" -eq 0 ] && echo 0 || echo 1)"

# --- case 6: the real legitimately-marker-bearing files stay green ---------
REAL="$WORK/real"
mkdir -p "$REAL/ci"
cp "$ROOT/ci/merge-appendonly.py" "$REAL/ci/merge-appendonly.py"
cp "$ROOT/ci/merge-appendonly-falsify.sh" "$REAL/ci/merge-appendonly-falsify.sh"
REAL_OUT="$(run_guard "$REAL" 2>&1)"
REAL_RC=$?
check "case 6: ci/merge-appendonly.py and its falsify script stay green" \
    "$([ "$REAL_RC" -eq 0 ] && echo 0 || echo 1)"
if [ "$REAL_RC" -ne 0 ]; then
    printf '%s\n' "$REAL_OUT" >&2
fi

# --- case 7: whole-file duplication, no markers at all ---------------------
DUP="$WORK/dup"
mkdir -p "$DUP/docs"
python3 -c "
unit = b'## 1. Section\n' + b'x' * 300 + b'\n'
open('$DUP/docs/DETERMINISM.md', 'wb').write(unit * 5)
"
DUP_OUT="$(run_guard "$DUP" 2>&1)"
DUP_RC=$?
check "case 7: a file duplicated 5x with no markers is refused" \
    "$([ "$DUP_RC" -ne 0 ] && echo 0 || echo 1)"
check "case 7: names it a whole-file duplicate, not a marker" \
    "$(printf '%s' "$DUP_OUT" | grep -cF 'WHOLE-FILE-DUPLICATE' >/dev/null && echo 0 || echo 1)"

# --- case 8: near-duplicate (one byte different) must stay green -----------
NEARDUP="$WORK/neardup"
mkdir -p "$NEARDUP/docs"
python3 -c "
half = b'## 1. Section\n' + b'x' * 300 + b'\n'
other = half[:-2] + b'y\n'
open('$NEARDUP/docs/DETERMINISM.md', 'wb').write(half + other)
"
NEARDUP_OUT="$(run_guard "$NEARDUP" 2>&1)"
NEARDUP_RC=$?
check "case 8: an almost-duplicated file (one byte off) stays green" \
    "$([ "$NEARDUP_RC" -eq 0 ] && echo 0 || echo 1)"

if [ "$FAIL" -eq 0 ]; then
    echo "CHECK-CONFLICT-MARKERS-FALSIFY GREEN — all four marker forms and whole-file duplication caught, clean/legitimate/near-duplicate trees stay green"
else
    echo "CHECK-CONFLICT-MARKERS-FALSIFY RED"
fi
exit "$FAIL"
