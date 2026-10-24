#!/usr/bin/env bash
# ci/check-vacuous-green-falsify.sh — show the vacuous-green guard going RED
# before anybody is asked to trust a green.
#
# # WHY EVERY CASE ASSERTS ON A STRING AND NOT ON AN EXIT CODE
#
# Two falsifiers on this project passed green on 2026-09-05 for reasons that
# had nothing to do with the thing they were testing: one mutated an EMPTY
# dict and reported the mutation caught, and one read a FAILED BUILD as a
# caught mutation. Both would have passed a `[ "$rc" -ne 0 ]` assertion.
#
# So every case here does three things and not one:
#
#   1. asserts the scratch tree is GREEN *before* the injection, so a case
#      cannot pass because the tree was already red;
#   2. asserts the red names THIS injection — the specific refusal sentence —
#      so a crash, a syntax error or an unrelated ratchet cannot be read as
#      the guard working;
#   3. removes the injection and asserts GREEN again, so a case cannot pass
#      because the guard has become stuck red.
#
# Case 5 is the one that matters most and it is the reason this file exists in
# the shape it does: **the census generator returning nothing at all.** Every
# comparison the guard makes is over the census's population, so an empty
# census makes all of them succeed. It is tested in two spellings that must
# not be conflated — a generator that runs and finds nothing (exit 1, the
# floor message) and a generator that cannot run at all (exit 2, REFUSED) —
# because a guard that answers both with the same word cannot tell a shrunken
# tree from a broken tool.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/vacgreen-falsify.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT

CASES=0
ASSERTS=0
FAILED=0

fail() { echo "  !! $*" >&2; FAILED=$((FAILED + 1)); }

# --- the scratch tree -------------------------------------------------------
# A copy, because every case edits `ci/` and one of them edits `ci/t0.sh`.
# `target`, `.git`, `oracle-work` and `data/vanilla` are excluded for the same
# reason `ci/t0-remote.sh` excludes them: they are large and nothing here
# reads them. The census scans source, not artefacts.
T="$SCRATCH/tree"
mkdir -p "$T"
rsync -a --exclude target --exclude .git --exclude oracle-work \
      --exclude 'data/vanilla' "$ROOT/" "$T/" 2>/dev/null || {
    echo "vacuous-green-falsify: REFUSED — could not build a scratch tree" >&2
    exit 2
}
[ -x "$T/ci/check-vacuous-green.py" ] || {
    echo "vacuous-green-falsify: REFUSED — the scratch tree has no guard in it" >&2
    exit 2
}

LAST_OUT=""
LAST_RC=0
run_guard() {
    # NEVER `run_guard | grep -q`. `grep -q` exits at the first match, the
    # producer takes SIGPIPE, and under `set -o pipefail` the pipeline reports
    # 141 — a passing assertion read as a failure. Captured once, greped from
    # the variable, which also halves the number of derivations this file runs.
    LAST_OUT="$( cd "$T" && python3 ci/check-vacuous-green.py 2>&1 )"
    LAST_RC=$?
}
says()     { printf '%s' "$LAST_OUT" | grep -cF -- "$1" >/dev/null; }
says_re()  { printf '%s' "$LAST_OUT" | grep -cE -- "$1" >/dev/null; }
rederive() { ( cd "$T" && python3 ci/vacuous-green-census.py --write >/dev/null 2>&1 ); }

# assert_green <label>
assert_green() {
    ASSERTS=$((ASSERTS + 1))
    run_guard
    if [ "$LAST_RC" -ne 0 ]; then
        fail "$1: expected GREEN, got exit $LAST_RC: $(printf '%s' "$LAST_OUT" | head -3)"
        return 1
    fi
    # A green that scanned nothing is the failure this whole file is about.
    ASSERTS=$((ASSERTS + 1))
    says_re 'VACUOUS-GREEN — [0-9]+ scripts scanned' || {
        fail "$1: exit 0 but the guard did not report a scanned population"
        return 1
    }
    return 0
}

# assert_red <label> <expected-exit> <substring>
assert_red() {
    ASSERTS=$((ASSERTS + 1))
    run_guard
    if [ "$LAST_RC" -ne "$2" ]; then
        fail "$1: expected exit $2, got $LAST_RC: $(printf '%s' "$LAST_OUT" | head -3)"
        return 1
    fi
    ASSERTS=$((ASSERTS + 1))
    says "$3" || {
        fail "$1: exit $2 but the message does not name the injection ($3): $(printf '%s' "$LAST_OUT" | head -3)"
        return 1
    }
    return 0
}

echo "== case 0: the unmodified scratch tree is GREEN over a real population"
CASES=$((CASES + 1))
assert_green "case 0 control"

echo "== case 1: a NEW uninvoked check appears"
CASES=$((CASES + 1))
cat > "$T/ci/check-planted-orphan.py" <<'PY'
#!/usr/bin/env python3
"""A planted check that nothing invokes."""
import sys
if len(sys.argv) > 99:
    print("PLANTED RED — this never happens", file=sys.stderr)
    sys.exit(1)
sys.exit(0)
PY
rederive
assert_red "case 1" 1 "uninvoked ci/check-planted-orphan.py"
rm -f "$T/ci/check-planted-orphan.py"
rederive
assert_green "case 1 restored"

echo "== case 2: an existing declared instance is silently repaired, declaration left stale"
CASES=$((CASES + 1))
# `ci/movement-falsify.sh` is declared `uninvoked`. Give it a caller and the
# census stops reporting it; the declaration must then be refused as stale.
printf '\n# planted caller\n./ci/movement-falsify.sh || true\n' >> "$T/ci/lanes.sh"
rederive
assert_red "case 2" 1 "uninvoked ci/movement-falsify.sh"
ASSERTS=$((ASSERTS + 1))
says "no longer finds" || fail "case 2: red did not name it as a STALE declaration"
# The same red must NOT read as a new instance — the two arms say opposite
# things to whoever has to act on it.
ASSERTS=$((ASSERTS + 1))
says "does not declare" && fail "case 2: a stale declaration was reported as a new instance"
git -C "$T" checkout -- ci/lanes.sh 2>/dev/null || \
    sed -i '/^\.\/ci\/movement-falsify\.sh || true$/d; /^# planted caller$/d' "$T/ci/lanes.sh"
rederive
assert_green "case 2 restored"

echo "== case 3: the census generator returns nothing at all"
CASES=$((CASES + 1))
cp "$T/ci/vacuous-green-census.py" "$SCRATCH/census.bak"
cat > "$T/ci/vacuous-green-census.py" <<'PY'
#!/usr/bin/env python3
"""A generator that runs, succeeds, and finds nothing."""
import sys
print("""[meta]
scripts_scanned = 0
verdict_scripts = 0
uninvoked_verdict_scripts = 0
green_on_refusal_scripts = 0
green_on_refusal_sites = 0
errexit_dead_scripts = 0
errexit_dead_captures = 0
no_population_floor = 0
""")
sys.exit(0)
PY
assert_red "case 3" 1 "has stopped finding anything"
ASSERTS=$((ASSERTS + 1))
# It must NOT be reported as "every instance declared" — an empty population
# satisfies every set comparison the guard makes, which is the whole point.
says "every one declared" && fail "case 3: an empty census passed the declaration comparison"
cp "$SCRATCH/census.bak" "$T/ci/vacuous-green-census.py"
assert_green "case 3 restored"

echo "== case 4: the census generator cannot run at all"
CASES=$((CASES + 1))
printf 'this is not python\n' > "$T/ci/vacuous-green-census.py"
assert_red "case 4" 2 "the census generator failed"
ASSERTS=$((ASSERTS + 1))
# A broken tool and a shrunken tree are different findings with opposite
# fixes, so they must not share an exit code.
says "has stopped finding anything" && \
    fail "case 4: a broken generator was reported as an empty tree"
cp "$SCRATCH/census.bak" "$T/ci/vacuous-green-census.py"
assert_green "case 4 restored"

echo "== case 5: a NEW green-on-refusal site in a check a gate runs"
CASES=$((CASES + 1))
cp "$T/ci/check-conflict-markers.py" "$SCRATCH/markers.bak"
cat >> "$T/ci/check-conflict-markers.py" <<'PY'


def _planted_refusal():
    print("check-conflict-markers: SKIPPED — cannot read this tree")
    return 0
PY
rederive
assert_red "case 5" 1 "green-on-refusal ci/check-conflict-markers.py"
cp "$SCRATCH/markers.bak" "$T/ci/check-conflict-markers.py"
rederive
assert_green "case 5 restored"

echo "== case 6: a dead exit handler under set -e"
CASES=$((CASES + 1))
cp "$T/ci/t0.sh" "$SCRATCH/t0.bak"
printf '\npython3 ci/check-figures.py; PLANTED_RC=$?\ncase $PLANTED_RC in 0) ;; *) exit 1 ;; esac\n' >> "$T/ci/t0.sh"
rederive
assert_red "case 6" 1 "ci/t0.sh:"
ASSERTS=$((ASSERTS + 1))
says "that cannot run, under an active" || \
    fail "case 6: the red did not name the shape, only the file"
ASSERTS=$((ASSERTS + 1))
# It must not be offered as something a line in DECLARED could clear.
says "add ONE LINE to DECLARED" && \
    fail "case 6: an errexit-dead handler was offered as declarable"
cp "$SCRATCH/t0.bak" "$T/ci/t0.sh"
rederive
assert_green "case 6 restored"

echo "== case 7: the committed census is stale"
CASES=$((CASES + 1))
sed -i 's/^uninvoked_verdict_scripts = .*/uninvoked_verdict_scripts = 999/' \
    "$T/ci/checks/vacuous-green.toml"
assert_red "case 7" 1 "the committed census is stale"
rederive
assert_green "case 7 restored"

echo "== case 8: two new instances, one declared, one not — the ceiling still holds"
CASES=$((CASES + 1))
for n in a b; do
cat > "$T/ci/check-planted-$n.py" <<'PY'
#!/usr/bin/env python3
import sys
if len(sys.argv) > 99:
    print("PLANTED RED", file=sys.stderr)
    sys.exit(1)
sys.exit(0)
PY
done
# Declare only one of the two, the way somebody clearing a red one line at a
# time would.
sed -i 's|^DECLARED: dict\[str, str\] = {|DECLARED: dict[str, str] = {\n    "uninvoked ci/check-planted-a.py": "planted",|' \
    "$T/ci/check-vacuous-green.py"
rederive
assert_red "case 8" 1 "uninvoked ci/check-planted-b.py"
ASSERTS=$((ASSERTS + 1))
says "uninvoked ci/check-planted-a.py" && \
    fail "case 8: the declared plant was reported as undeclared"
rm -f "$T/ci/check-planted-a.py" "$T/ci/check-planted-b.py"
sed -i '/"uninvoked ci\/check-planted-a.py": "planted",/d' "$T/ci/check-vacuous-green.py"
rederive
assert_green "case 8 restored"

echo
if [ "$FAILED" -ne 0 ]; then
    echo "vacuous-green-falsify: RED — $FAILED of $ASSERTS assertions failed over $CASES cases" >&2
    exit 1
fi
echo "vacuous-green-falsify GREEN — $CASES cases, $ASSERTS assertions, every injection refused by name and every removal restored to green"
