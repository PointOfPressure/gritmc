#!/usr/bin/env bash
# ===== AGENT INTERFACE =====
#
#     ./ci/floor-stamp-vocabulary-falsify.sh    # ~1s, no server, no lock,
#                                               # and NO DEPENDENCE ON THE BOX
# ===== end interface; everything below is why =====
#
# Shows `ci/floor-stamp-vocabulary.py` refusing every way it can be pointed at
# the wrong recordings, and shows the stamp being exactly one key wide.
#
# A vocabulary derived from the WRONG recordings is worse than no vocabulary:
# it makes a floor look comparable to a run it is not comparable with, which is
# the single failure §50's three outcomes exist to prevent. So the refusals are
# the product here and the successful stamp is the control.
#
# # THE FIXTURES ARE SYNTHETIC, AND THAT IS THE POINT
#
# The first version of this file read the real ensemble out of
# `~/rustmc-cov2/oracle-work/gameplay-selfcheck/` and printed `SKIP` when it was
# not on the box. **That is exactly the defect this branch was written to
# criticise.** §50.5 records `ci/corpus-floor-falsify.sh` case 1 as
# "conditionally armed, and nothing says so" — a case whose coverage is a
# function of leftover working state, green on a clean tree for a reason that
# has nothing to do with the code. Writing a second one of those, in the commit
# that names the first, would have been the joke telling itself.
#
# So everything below is built in a `mktemp -d`: two three-line traces and a
# floor file whose `[[recording]]` rows carry their real sha256s, computed here
# rather than pasted. The suite is armed in every tree, on every box, forever,
# and it tests the script's logic rather than the box's tidiness.
#
# The last case is the one that would otherwise never be tested. A stamp that
# also rewrote a measured figure would look completely correct — same file,
# same ensemble, plausible numbers — and would be a re-measurement published
# under the old `projection` id. So case 4 diffs the whole file and asserts
# that the ONLY difference is the `vocabulary` block.
set -u
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 2
ROOT=$PWD
fail=0; pass=0

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT

# --- two synthetic recordings ---------------------------------------------
# `trace_vocabulary.read` refuses a comparison report, a non-trace and an EMPTY
# vocabulary, so these have to be real enough to pass all three: a `recorder`
# key, a `scenarios` list, and at least one observation carrying a field.
# `r2` carries one pair `r1` does not, so the union is a strict superset of
# either — which is what makes case 2's "a subset understates the union"
# something this suite can actually demonstrate rather than assert.
cat >"$work/r1.json" <<'JSON'
{"recorder": "rec:0000000000000001", "scenarios": [
  {"name": "synthetic-a", "obs": [
    {"who": "actor", "step": 0, "kind": "add_entity",
     "fields": {"at": [0,0,0], "kind_id": 71, "movement": [0,0,0]}}]}]}
JSON
cat >"$work/r2.json" <<'JSON'
{"recorder": "rec:0000000000000001", "scenarios": [
  {"name": "synthetic-a", "obs": [
    {"who": "actor", "step": 0, "kind": "add_entity",
     "fields": {"at": [0,0,0], "kind_id": 71, "movement": [0,0,0], "y_rot": 3}}]}]}
JSON
cat >"$work/foreign.json" <<'JSON'
{"recorder": "rec:00000000000000ff", "scenarios": [
  {"name": "synthetic-z", "obs": [
    {"who": "actor", "step": 0, "kind": "sound", "fields": {"pitch": 1.0}}]}]}
JSON

H1=$(sha256sum "$work/r1.json" | cut -d' ' -f1)
H2=$(sha256sum "$work/r2.json" | cut -d' ' -f1)

# --- the pre-stamp floor file ---------------------------------------------
# Shaped like `ci/checks/gameplay-floor.toml`: a `[floor]` table with a
# `recorders` line to write beside, some measured figures that must not move,
# and one `[[recording]]` row per synthetic trace.
BARE=$work/bare.toml
cat >"$BARE" <<TOML
[floor]
ensemble = "synthetic-falsify-n2"
projection = "proj:0000000000000000"
runs = 2
pairs = 1
scenarios = 1
floor = 1
floor_reference = 1
recorders = 1

[[recording]]
id = "r1"
sha256 = "$H1"

[[recording]]
id = "r2"
sha256 = "$H2"
TOML

run () { python3 "$ROOT/ci/floor-stamp-vocabulary.py" "$@" 2>&1; }

expect () {  # name rc needle -- args...
    local name=$1 want=$2 needle=$3; shift 3; [ "${1:-}" = "--" ] && shift
    local out rc; out=$(run "$@"); rc=$?
    if [ "$rc" != "$want" ]; then echo "  FAIL $name: exit $rc wanted $want"; fail=$((fail+1)); return; fi
    if ! printf '%s' "$out" | grep -c -- "$needle" >/dev/null; then
        echo "  FAIL $name: no '$needle' in output"; fail=$((fail+1)); return; fi
    echo "  ok   $name"; pass=$((pass+1))
}

cp "$BARE" "$work/f.toml"
echo "case 0 — --check says NO VOCABULARY on the pre-stamp shape"
expect "0 NO VOCABULARY" 1 "NO VOCABULARY" -- --check "$work/f.toml"

echo "case 1 — a recording that is not in the ensemble"
cp "$BARE" "$work/f.toml"
expect "1 foreign recording refused" 1 "is not one of" -- "$work/f.toml" \
    "$work/r1.json" "$work/foreign.json"

echo "case 2 — a proper subset of the ensemble"
# The direction that matters: a subset UNDERSTATES the union, and an
# understated baseline is what produces a false VOCABULARY DRIFT on the next
# corpus run — a check crying wolf is a check switched off within a week.
# `r2` carries `add_entity.y_rot` and `r1` does not, so stamping from `r1`
# alone would write a vocabulary the ensemble's own second run drifts from.
cp "$BARE" "$work/f.toml"
expect "2 subset refused" 1 "were not offered" -- "$work/f.toml" "$work/r1.json"

echo "case 3 — the whole ensemble stamps, and --check then passes"
cp "$BARE" "$work/f.toml"
expect "3 stamped" 0 "stamped 4" -- "$work/f.toml" "$work/r1.json" "$work/r2.json"
expect "3b --check passes" 0 "vocabulary 4 pair(s)" -- --check "$work/f.toml"

echo "case 3c — the stamped union is r1's pairs PLUS r2's, not either alone"
if grep -q '"add_entity.y_rot",' "$work/f.toml" && grep -q '"add_entity.movement",' "$work/f.toml"; then
    echo "  ok   3c the union is a union"; pass=$((pass+1))
else
    echo "  FAIL 3c the stamped vocabulary is not the union of the two runs"
    grep -A6 '^vocabulary' "$work/f.toml"; fail=$((fail+1))
fi

echo "case 4 — the stamp is exactly one key wide"
if diff <(grep -v '^vocabulary = \[$' "$work/f.toml" | grep -vE '^    "[^"]+",$' | grep -v '^\]$') "$BARE" >/dev/null; then
    echo "  ok   4 no measured figure moved"; pass=$((pass+1))
else
    echo "  FAIL 4 the stamp changed something other than the vocabulary:"
    diff <(grep -v '^vocabulary = \[$' "$work/f.toml" | grep -vE '^    "[^"]+",$' | grep -v '^\]$') "$BARE" | head -10
    fail=$((fail+1))
fi

echo "case 5 — a second stamp over a present vocabulary is refused"
expect "5 no overwrite" 1 "already declares a vocabulary" -- "$work/f.toml" \
    "$work/r1.json" "$work/r2.json"

echo "case 6 — a floor file with no [[recording]] rows cannot be stamped at all"
# Without them nothing here can check that the recordings ARE the ensemble, and
# a stamp on trust is the failure the sha256 guard exists to prevent.
sed '/^\[\[recording\]\]/,$d' "$BARE" >"$work/norows.toml"
expect "6 no rows refused" 1 "lists no \[\[recording\]\] rows" -- "$work/norows.toml" \
    "$work/r1.json" "$work/r2.json"

echo
echo "floor-stamp-vocabulary-falsify: $pass passed, $fail failed"
[ "$fail" = 0 ] || exit 1
