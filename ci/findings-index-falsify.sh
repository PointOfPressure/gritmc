#!/usr/bin/env bash
# Prove `ci/findings-index.py` can go red, and that it does not go red on the
# four things it must not.
#
#     ./ci/findings-index-falsify.sh      # ~2 s, no build, no jar, no box
#
# THE CASE THAT IS THE WHOLE ARGUMENT is the pair `unindexed` / `indexed`: one
# branch, one added `WITHDRAWN` line, red before the index line is appended and
# green after. A check whose red state is never demonstrated is this project's
# central defect class, and the tripwire shape here — fires only on a
# vocabulary — is exactly the shape that goes quiet without anyone noticing.
#
# THE NEGATIVE CONTROLS ARE HALF THE FILE, because the obvious wrong version of
# this check is "every docs edit must append to the index", which would make
# the index a toll on editing documents rather than a record of findings:
#
#   ordinary-edit  a docs line with no marker, no index line -> GREEN
#   lowercase      `withdrawn` in ordinary prose             -> GREEN
#   cold-start     a marker inside docs/cold-start/          -> GREEN
#   index-only     appending to the index and nothing else   -> GREEN
#
# `lowercase` is the one that earns its place: the vocabulary is case-sensitive
# on purpose, because `withdrawn` in a sentence is prose and `WITHDRAWN` is this
# project marking a claim as no longer standing. Without this case the rule
# would be a rule against a word.
#
# The NO-GIT path is asserted too. `ci/t0.sh` runs in an rsynced tree with no
# `.git`, so that is where this check spends most of its life, and a check that
# quietly does nothing there is worth nothing.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FAIL=0
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT

check() {
    local label="$1" got="$2" want="$3"
    if [ "$got" = "$want" ]; then echo "ok   $label"
    else echo "FAIL $label (exit $got, wanted $want)"; FAIL=1; fi
}

# One fixture repo, rebuilt per case so each starts from the same base.
setup() {
    R="$TMP/r$RANDOM$RANDOM"
    mkdir -p "$R/ci" "$R/docs/cold-start"
    cp "$ROOT/ci/findings-index.py" "$R/ci/"
    git -C "$R" init -q -b main
    git -C "$R" config user.email "falsify@example.invalid"
    git -C "$R" config user.name "findings-index-falsify"
    printf '# PARITY\n\nbase prose.\n'  > "$R/docs/PARITY.md"
    printf '# FINDINGS\n\n* a base finding.\n' > "$R/docs/FINDINGS.md"
    printf '# cold start\n' > "$R/docs/cold-start/x.md"
    git -C "$R" add -A && git -C "$R" commit -q -m base
    git -C "$R" checkout -q -b lane
}
run() { python3 "$R/ci/findings-index.py" --check >"$TMP/out" 2>&1; echo $?; }

echo "-- the arm that must be able to refuse, and the repair --"
setup
printf 'The r=16 headline is WITHDRAWN as a level.\n' >> "$R/docs/PARITY.md"
git -C "$R" commit -q -am withdraw
check "unindexed: a WITHDRAWN line with no index entry is RED" "$(run)" 1
grep -q 'records a finding and does not index it' "$TMP/out" \
    || { echo "FAIL unindexed: the RED does not say what is wrong"; FAIL=1; }
grep -q 'WITHDRAWN' "$TMP/out" \
    || { echo "FAIL unindexed: the RED does not name the marker that tripped it"; FAIL=1; }

printf '* the r=16 headline was withdrawn as a level.\n' >> "$R/docs/FINDINGS.md"
git -C "$R" commit -q -am index
check "indexed:   the same branch, one index line later, is GREEN" "$(run)" 0

echo "-- the other markers and phrases fire too --"
for m in RETRACTED REFUTED CORRECTED SUPERSEDED DISQUALIFIED RESCINDED; do
    setup
    printf 'This figure is %s.\n' "$m" >> "$R/docs/PARITY.md"
    git -C "$R" commit -q -am m
    check "marker $m is RED unindexed" "$(run)" 1
done
setup
printf 'The instrument measured the wrong quantity.\n' >> "$R/docs/PARITY.md"
git -C "$R" commit -q -am phrase
check "phrase 'measured the wrong' is RED unindexed" "$(run)" 1

echo "-- negative controls (these must stay green) --"
setup
printf 'The nether window now covers y 0-127.\n' >> "$R/docs/PARITY.md"
git -C "$R" commit -q -am ordinary
check "ordinary-edit: a docs line with no marker is GREEN" "$(run)" 0

setup
printf 'The claim was withdrawn in the usual way, as prose describes.\n' >> "$R/docs/PARITY.md"
git -C "$R" commit -q -am lower
check "lowercase:    'withdrawn' in prose is GREEN (the vocabulary is cased)" "$(run)" 0

setup
printf 'Earlier reading WITHDRAWN, see the branch note.\n' >> "$R/docs/cold-start/x.md"
git -C "$R" commit -q -am cold
check "cold-start:   a marker in docs/cold-start/ is GREEN" "$(run)" 0

setup
printf '* a finding with no doc edit behind it.\n' >> "$R/docs/FINDINGS.md"
git -C "$R" commit -q -am indexonly
check "index-only:   appending to the index alone is GREEN" "$(run)" 0

echo "-- the no-git path states itself rather than passing quietly --"
N="$TMP/nogit"; mkdir -p "$N/ci"; cp "$ROOT/ci/findings-index.py" "$N/ci/"
python3 "$N/ci/findings-index.py" --check >"$TMP/out" 2>&1; rc=$?
check "no-git: exits 0" "$rc" 0
if grep -q 'FINDINGS-INDEX NO-GIT' "$TMP/out"; then
    echo "ok   no-git: says NO-GIT and why, rather than printing GREEN"
else
    echo "FAIL no-git: exited 0 without saying it could not run"; FAIL=1
fi

if [ "$FAIL" -eq 0 ]; then
    echo "FINDINGS-INDEX-FALSIFY GREEN — 15 cases: every marker and phrase reds"
    echo "  unindexed, the same branch greens on one appended line, an ordinary"
    echo "  docs edit and a lowercase 'withdrawn' and a cold-start note and an"
    echo "  index-only commit all stay green, and the no-git path announces itself"
else
    echo "FINDINGS-INDEX-FALSIFY RED"
fi
exit "$FAIL"
