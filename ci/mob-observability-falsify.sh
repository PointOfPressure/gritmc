#!/usr/bin/env bash
# mob-observability-falsify.sh — inject faults, assert ci/mob-observability.py catches each.
#
# The gate here refuses a mob-bearing check that the blindness ledger does not
# cover. Two things have to be live for that to mean anything: the detector
# that decides a check puts a mob in the world, and the classifier that
# decides an `observe` list contains a behaviour channel. A dead one of either
# makes the gate pass everything, quietly, which is the shape this project has
# now found in four instruments.
#
# Seven assertions, all in a scratch corpus, none in the worktree:
#   1  the clean tree is green
#   2  a new spawn-egg check with no ledger entry is REFUSED and named
#   3  a new `summon` check is refused too — the other way to make a mob
#   4  a check that makes no mob is NOT flagged (the negative control: a
#      detector that fires on everything would pass 2 and 3 for free)
#   5  deleting the ledger entry for the existing check refuses
#   6  a ledger naming a check the corpus has dropped refuses as stale
#   7  the behaviour classifier is live — the corpus's four behaviour-observing
#      checks are found, and emptying the BEHAVIOUR set takes that count to 0
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
FAIL=0
note() { printf '  %-5s %s\n' "$1" "$2"; }
ok()   { note "ok" "$1"; }
bad()  { note "FAIL" "$1"; FAIL=1; }

mkdir -p "$SCRATCH/ci/checks/gameplay"
cp "$HERE/mob-observability.py" "$SCRATCH/ci/"
cp "$HERE/checks/mob-blind.toml" "$SCRATCH/ci/checks/"
cp "$HERE/checks/gameplay/"*.toml "$SCRATCH/ci/checks/gameplay/"
OBS="$SCRATCH/ci/mob-observability.py"
LED="$SCRATCH/ci/checks/mob-blind.toml"
COR="$SCRATCH/ci/checks/gameplay"

echo "== 1: the clean scratch corpus is green"
if python3 "$OBS" --check >/dev/null 2>&1; then
    ok "--check green before any fault"
else
    bad "--check red on a clean copy — every later assertion is vacuous"
fi

echo "== 2: a new spawn-egg check with no ledger entry is refused"
cat > "$COR/999-falsify-a-zombie-appears.toml" <<'T'
name = "falsify-a-zombie-appears"
about = "injected by ci/mob-observability-falsify.sh; never loaded by the corpus"
observe = ["add_entity", "set_entity_data"]
[[setup]]
do = "hold"
item = "minecraft:zombie_spawn_egg"
[[step]]
do = "place"
on = [1, -1, 0]
face = "up"
T
ERR="$(python3 "$OBS" --check 2>&1 >/dev/null)"; RC=$?
if [ $RC -ne 0 ]; then ok "refused"; else bad "a new mob check went unrefused"; fi
if printf '%s' "$ERR" | grep -c "falsify-a-zombie-appears" >/dev/null; then
    ok "named the check"
else
    bad "refused without naming the check"
fi
rm -f "$COR/999-falsify-a-zombie-appears.toml"

echo "== 3: a summon check is refused too"
cat > "$COR/999-falsify-summon.toml" <<'T'
name = "falsify-summon"
about = "injected by ci/mob-observability-falsify.sh"
observe = ["add_entity"]
[[step]]
do = "command"
line = "summon minecraft:pig 1 -1 0"
T
python3 "$OBS" --check >/dev/null 2>&1; RC=$?
if [ $RC -ne 0 ]; then ok "refused a summon"; else bad "summon is not detected as a mob maker"; fi
rm -f "$COR/999-falsify-summon.toml"

echo "== 4: a check that makes no mob is not flagged (negative control)"
cat > "$COR/999-falsify-plain.toml" <<'T'
name = "falsify-plain"
about = "injected by ci/mob-observability-falsify.sh; places a block, no mob"
observe = ["block_update"]
[[setup]]
do = "hold"
item = "minecraft:stone"
[[step]]
do = "place"
on = [1, -1, 0]
face = "up"
T
if python3 "$OBS" --check >/dev/null 2>&1; then
    ok "not flagged — the detector discriminates"
else
    bad "a mobless check was flagged; the detector fires on everything and 2 and 3 prove nothing"
fi
rm -f "$COR/999-falsify-plain.toml"

echo "== 5: deleting the ledger entry refuses"
cp "$LED" "$SCRATCH/led.orig"
grep -v 'spawn-egg-makes-a-mob' "$SCRATCH/led.orig" > "$LED"
if python3 "$OBS" --check >/dev/null 2>&1; then
    bad "a missing ledger entry went unrefused"
else
    ok "refused"
fi
cp "$SCRATCH/led.orig" "$LED"

echo "== 6: a ledger entry with no check refuses as stale"
printf '\n[[blind]]\nname = "falsify-a-check-that-is-gone"\n' >> "$LED"
ERR="$(python3 "$OBS" --check 2>&1 >/dev/null)"; RC=$?
if [ $RC -ne 0 ] && printf '%s' "$ERR" | grep -c "no longer has" >/dev/null; then
    ok "refused as stale"
else
    bad "a stale ledger entry went unrefused"
fi
cp "$SCRATCH/led.orig" "$LED"

echo "== 7: the behaviour classifier is live"
N="$(python3 "$OBS" 2>/dev/null | sed -n 's/.*checks observing a behaviour channel *//p')"
if [ "${N:-0}" -ge 1 ] 2>/dev/null; then
    ok "found $N behaviour-observing checks in the corpus"
else
    bad "found none — the classifier is dead and 'no mob check observes behaviour' is vacuous"
fi
# Blank the WHOLE set rather than one channel. One channel does not do it and
# the first version of this assertion failed for that reason, correctly: the
# four checks overlap — two of them observe `entity_position` AND
# `move_entity`, so deleting either leaves the count at four. An assertion
# that moves only when the classifier is entirely dead is weaker than one that
# moves per channel, and it is the one that is actually true here.
python3 - "$OBS" <<'PY'
import re,sys
p=sys.argv[1]; s=open(p).read()
s=re.sub(r'BEHAVIOUR = \{[^}]*\}', 'BEHAVIOUR = {"falsify_no_such_kind"}', s, count=1)
open(p,"w").write(s)
PY
N2="$(python3 "$OBS" 2>/dev/null | sed -n 's/.*checks observing a behaviour channel *//p')"
if [ "${N2:-99}" = "0" ] 2>/dev/null; then
    ok "emptying the channel set took the count $N -> 0"
else
    bad "the count did not fall to 0 ($N -> $N2); it is not reading the observe lists"
fi

echo
if [ $FAIL -eq 0 ]; then
    echo "mob-observability-falsify: all assertions held"
else
    echo "mob-observability-falsify: RED — an injected fault went unrefused" >&2
fi
exit $FAIL
