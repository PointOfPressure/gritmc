#!/usr/bin/env bash
# survival-substrate-assert-falsify.sh — prove the substrate assertion can fail.
#
#     ./ci/survival-substrate-assert-falsify.sh
#
# The assertion it covers is the only one in `ci/t2-gameplay-survival.sh` whose
# absence is invisible: a game mode that did not take produces a recording that
# looks exactly like a green everywhere else in the pipeline. So it gets a
# falsifier with a synthetic recording for each outcome, rather than being
# trusted because it is short.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/ci/survival-substrate-assert.py"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASS=0; FAIL=0
ok()  { printf '  ok    %s\n' "$1"; PASS=$((PASS + 1)); }
bad() { printf '  FAIL  %s\n' "$1"; FAIL=$((FAIL + 1)); }

rec() {  # rec <file> <kind>...   a recording carrying one obs per kind named
    local f="$1"; shift
    python3 - "$f" "$@" <<'PY'
import json, sys
f, kinds = sys.argv[1], sys.argv[2:]
obs = [{"who": "actor", "step": 0, "at_ms": 0, "kind": k, "fields": {}} for k in kinds]
json.dump({"target": "synthetic", "version": "26.2", "protocol": 0,
           "recorder": "falsify",
           "scenarios": [{"name": "synthetic", "anchor": [0, 0, 0],
                          "spawn": [0.0, 0.0, 0.0], "obs": obs}]}, open(f, "w"))
PY
}

# --- 1: a creative-substrate recording is REFUSED ------------------------
rec "$WORK/creative.json" block_update sound block_changed_ack
if OUT="$("$TOOL" "$WORK/creative.json" 2>&1)"; then
    bad "1  a recording with no gated kind was ACCEPTED — the guard is inert"
else
    [ "$?" = 4 ] && grep -q 'SURVIVAL PASS REFUSED' <<<"$OUT" \
        && ok "1  a creative-substrate recording is refused with exit 4" \
        || bad "1  refused, but not with exit 4 and the explanation"
fi

# --- 2: each gated kind ALONE is enough to pass --------------------------
allok=1
for k in damage_event set_health block_destruction set_experience; do
    rec "$WORK/one.json" block_update "$k"
    "$TOOL" "$WORK/one.json" >/dev/null 2>&1 || { bad "2  $k alone did not satisfy the guard"; allok=0; }
done
[ "$allok" = 1 ] && ok "2  each of the four gated kinds alone satisfies the guard"

# --- 3: a near-miss kind must NOT satisfy it -----------------------------
# `entity_event` and `animate` arrive on the creative substrate too — a guard
# that accepted them would pass a creative recording.
rec "$WORK/near.json" entity_event animate sound
if "$TOOL" "$WORK/near.json" >/dev/null 2>&1; then
    bad "3  entity_event/animate satisfied the guard — those arrive under creative"
else
    ok "3  kinds that arrive under creative do not satisfy the guard"
fi

# --- 4: an unreadable file refuses rather than passing -------------------
echo 'not json' > "$WORK/bad.json"
OUT="$("$TOOL" "$WORK/bad.json" 2>&1)"; RC=$?
if [ "$RC" = 2 ] && grep -q 'cannot read' <<<"$OUT"; then
    ok "4  an unreadable recording refuses with exit 2, not a pass"
else
    bad "4  an unreadable recording did not refuse (rc=$RC)"
fi

# --- 5: a missing file refuses -------------------------------------------
OUT="$("$TOOL" "$WORK/nope.json" 2>&1)"; RC=$?
[ "$RC" = 2 ] && ok "5  a missing recording refuses with exit 2" \
              || bad "5  a missing recording did not refuse (rc=$RC)"

# --- 6: an empty scenario list is a refusal, not a pass ------------------
python3 -c "
import json,sys
json.dump({'target':'x','version':'26.2','protocol':0,'recorder':'f','scenarios':[]},
          open('$WORK/empty.json','w'))"
if "$TOOL" "$WORK/empty.json" >/dev/null 2>&1; then
    bad "6  a recording with no scenarios at all was ACCEPTED"
else
    ok "6  a recording with no scenarios is refused"
fi

printf '\nsurvival-substrate-assert-falsify: %d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
