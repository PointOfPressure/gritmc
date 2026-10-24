#!/usr/bin/env bash
# substrate-gate-audit-falsify.sh — prove ci/substrate-gate-audit.py can still fail.
#
#     ./ci/substrate-gate-audit-falsify.sh
#
# An instrument that has never been seen to go red is an unfalsifiable check,
# which is this repo's central defect class. Every case here **breaks the tool
# or its inputs on a copy** and asserts the specific red, then asserts the
# unbroken tool is green on the same inputs.
#
# The copy matters: nothing here edits `ci/substrate-gate-audit.py` in the tree,
# so a failure midway cannot leave a sabotaged instrument behind. Same shape as
# `CHAIN_FALSIFY_SRC` in `ci/chain-falsify.sh`.
#
# # What each case is defending
#
#   1  a gate whose OFFSET moved              -> MOVED, not a silent pass
#   2  a gate whose bytecode is GONE           -> GONE, distinct from MOVED
#   3  a return_at that is not a return        -> NO-RETURN
#   4  a never-recorded kind with no reading    -> UNATTRIBUTED, and a red
#   5  no recordings matched                    -> refuses; never prints a zero
#                                                  without a denominator
#   6  a sender that constructs no packet       -> NO PACKET CONSTRUCTED
#   7  the jar absent                           -> refuses with the path
#   8  a kind that IS recorded                  -> must NOT be listed as never
#
# Cases 1-3 are three different ways one row can rot and they are deliberately
# not collapsed: "the offset moved" and "the field is gone" want different
# repairs, and a tool that says only "red" makes the reader do the bisect.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/ci/substrate-gate-audit.py"
WORK="$(mktemp -d)"
# The copies live under ci/ rather than in $WORK, because the tool resolves the
# jar and trace.rs relative to its own path — a copy in /tmp would refuse with
# "pinned jar absent" and every case would pass for the wrong reason. The trap
# removes them on any exit, including a failure midway.
COPYDIR="$ROOT/ci"
trap 'rm -rf "$WORK"; rm -f "$COPYDIR"/.falsify-sga-*.py' EXIT

PASS=0
FAIL=0

ok()   { printf '  ok    %s\n' "$1"; PASS=$((PASS + 1)); }
bad()  { printf '  FAIL  %s\n' "$1"; FAIL=$((FAIL + 1)); }

# A recordings glob with exactly one small synthetic file, so the cases that
# are about the gates do not depend on what other lanes have on the box.
mkdir -p "$WORK/recs"
cat > "$WORK/recs/vanilla.json" <<'EOF'
{"target":"synthetic","version":"26.2","protocol":0,"recorder":"falsify",
 "scenarios":[{"name":"synthetic","anchor":[0,0,0],"spawn":[0,0,0],
 "obs":[{"who":"actor","step":0,"at_ms":0,"kind":"block_update","fields":{}}]}]}
EOF
GLOB="$WORK/recs/*.json"

run() {  # run <script> [args...]  -> stdout+stderr in $OUT, status in $RC
    OUT="$("$@" 2>&1)"; RC=$?
}

# --- baseline: the real tool, unbroken, on the real jar -------------------
run "$TOOL" --gates-only
if [ "$RC" -eq 0 ] && ! grep -qE 'MOVED|GONE|NO-RETURN' <<<"$OUT"; then
    ok "baseline: every declared gate still verifies against the pinned jar"
else
    bad "baseline: the tool is ALREADY red on the pinned jar — repair it before
        trusting any case below. Output:
$OUT"
fi

# --- 1: an offset that moved --------------------------------------------
sed 's/gate_at=15, return_at=32/gate_at=16, return_at=32/' "$TOOL" > "$COPYDIR/.falsify-sga-1.py"
chmod +x "$COPYDIR/.falsify-sga-1.py"
run python3 "$COPYDIR/.falsify-sga-1.py" --gates-only
if [ "$RC" -ne 0 ] && grep -q 'MOVED' <<<"$OUT"; then
    ok "1  a gate offset that moved is MOVED and a non-zero exit"
else
    bad "1  a moved gate offset did not produce MOVED (rc=$RC)"
fi

# --- 2: bytecode that is gone -------------------------------------------
sed 's|Abilities.invulnerable:Z|Abilities.notAFieldAnywhere:Z|' "$TOOL" > "$COPYDIR/.falsify-sga-2.py"
chmod +x "$COPYDIR/.falsify-sga-2.py"
run python3 "$COPYDIR/.falsify-sga-2.py" --gates-only
if [ "$RC" -ne 0 ] && grep -q 'GONE' <<<"$OUT"; then
    ok "2  a needle absent from the method is GONE, distinct from MOVED"
else
    bad "2  an absent needle did not produce GONE (rc=$RC)"
fi

# --- 3: a return_at that is not a return --------------------------------
sed 's/gate_at=4, return_at=10/gate_at=4, return_at=11/' "$TOOL" > "$COPYDIR/.falsify-sga-3.py"
chmod +x "$COPYDIR/.falsify-sga-3.py"
run python3 "$COPYDIR/.falsify-sga-3.py" --gates-only
if [ "$RC" -ne 0 ] && grep -q 'NO-RETURN' <<<"$OUT"; then
    ok "3  a return_at pointing at a non-return is NO-RETURN"
else
    bad "3  a bad return_at did not produce NO-RETURN (rc=$RC)"
fi

# --- 4: a never-recorded kind with no reading ---------------------------
# Drop damage_event's attribution; the synthetic recording carries only
# block_update, so damage_event is never-recorded and now unattributed.
python3 - "$TOOL" "$COPYDIR/.falsify-sga-4.py" <<'PY'
import re, sys
src = open(sys.argv[1]).read()
src = re.sub(r'\n    "damage_event": \("substrate".*?\),\n', '\n', src, count=1, flags=re.S)
open(sys.argv[2], 'w').write(src)
PY
chmod +x "$COPYDIR/.falsify-sga-4.py"
run python3 "$COPYDIR/.falsify-sga-4.py" --recordings "$GLOB"
if [ "$RC" -ne 0 ] && grep -q 'UNATTRIBUTED' <<<"$OUT"; then
    ok "4  a never-recorded kind with no reading is UNATTRIBUTED and a red"
else
    bad "4  an unattributed never-recorded kind was not flagged (rc=$RC)"
fi

# --- 5: no recordings matched -------------------------------------------
run "$TOOL" --recordings "$WORK/definitely-nothing-here/*.json"
if [ "$RC" -eq 2 ] && grep -q 'refusing to print a zero' <<<"$OUT"; then
    ok "5  an empty recordings glob refuses, rather than printing zeros"
else
    bad "5  an empty recordings glob did not refuse (rc=$RC)"
fi

# --- 6: a sender that constructs no packet ------------------------------
# Two sabotages, because there are two ways a sender row can be wrong and the
# second one is what the row's packet name was added for.
#
#   6a  the method builds no packet at all
#   6b  the method builds a DIFFERENT packet — ServerPlayer.doTick builds
#       several, so a check that only looked for `new ...Packet` would pass
#       set_health and set_experience while pointing at the wrong send.
sed 's|"broadcastDamageEvent",|"getDifficulty",|' "$TOOL" > "$COPYDIR/.falsify-sga-6.py"
chmod +x "$COPYDIR/.falsify-sga-6.py"
run python3 "$COPYDIR/.falsify-sga-6.py" --gates-only
if [ "$RC" -ne 0 ] && grep -q 'NOT CONSTRUCTED HERE' <<<"$OUT"; then
    ok "6a a sender whose method builds no packet is NOT CONSTRUCTED HERE"
else
    bad "6a a bogus sender was accepted (rc=$RC)"
fi

sed 's|"ClientboundSetHealthPacket"|"ClientboundSetExperiencePacket"|' \
    "$TOOL" > "$COPYDIR/.falsify-sga-6b.py"
chmod +x "$COPYDIR/.falsify-sga-6b.py"
run python3 "$COPYDIR/.falsify-sga-6b.py" --gates-only
if [ "$RC" -ne 0 ] && grep -q 'NOT CONSTRUCTED HERE' <<<"$OUT"; then
    bad "6b set_health pointing at the experience packet was REFUSED — but both
        are built in doTick, so this sabotage is not detectable by class name
        alone and the case is wrong, not the tool"
else
    ok "6b (both packets share doTick, so a class-name swap between them is NOT
        detectable here — recorded so nobody reads case 6a as stronger than it is)"
fi

# --- 7: the jar absent ---------------------------------------------------
run "$TOOL" --gates-only --jar "$WORK/no-such-server.jar"
if [ "$RC" -eq 2 ] && grep -q 'pinned jar absent' <<<"$OUT"; then
    ok "7  an absent jar refuses and names the path"
else
    bad "7  an absent jar did not refuse (rc=$RC)"
fi

# --- 8: a recorded kind must not be listed as never-recorded ------------
run "$TOOL" --recordings "$GLOB"
if grep -qE '^  block_update ' <<<"$OUT"; then
    bad "8  block_update IS in the synthetic recording and was listed as never-recorded"
else
    ok "8  a kind present in the recordings is not listed as never-recorded"
fi

# --- 9: the ability matrix cannot be read -------------------------------
# The matrix is what stops a lane naming the wrong lever, so a version that
# silently printed nothing would be worse than one that errors.
sed 's/method_body(gt, "updatePlayerAbilities")/method_body(gt, "noSuchMethodHere")/' \
    "$TOOL" > "$COPYDIR/.falsify-sga-9.py"
chmod +x "$COPYDIR/.falsify-sga-9.py"
run python3 "$COPYDIR/.falsify-sga-9.py" --gates-only
if [ "$RC" -ne 0 ] && grep -q 'COULD NOT READ' <<<"$OUT"; then
    ok "9  an unreadable ability matrix is COULD NOT READ, not a silent skip"
else
    bad "9  an unreadable ability matrix was skipped silently (rc=$RC)"
fi

# --- 10: a substrate verb whose gate vanished ---------------------------
sed 's|gate_needle="Field net/minecraft/world/entity/player/Abilities.instabuild:Z"|gate_needle="Field net/minecraft/world/entity/player/Abilities.goneAway:Z"|' \
    "$TOOL" > "$COPYDIR/.falsify-sga-10.py"
chmod +x "$COPYDIR/.falsify-sga-10.py"
run python3 "$COPYDIR/.falsify-sga-10.py" --gates-only
if [ "$RC" -ne 0 ] && grep -q 'GATE GONE' <<<"$OUT"; then
    ok "10 a substrate verb whose gate vanished is GATE GONE"
else
    bad "10 a vanished verb gate was not caught (rc=$RC)"
fi

# --- 11: a coverage verb that ACQUIRED a creative gate ------------------
# THE DIRECTION THAT ROTS SILENTLY, and the one the needle set exists for.
# `use_item`'s zero is a coverage gap only while `useItem` carries no
# creative-lever gate. Point the row at a method that HAS one and the row must
# flip, whichever spelling the gate happens to use. Three spellings, three
# different methods, because a needle set that only caught `instabuild` would
# look identical here on the one case that used it.
for m in handleBlockBreakAction isCreative destroyBlock; do
    sed "s|        method=\"useItem\",|        method=\"$m\",|" \
        "$TOOL" > "$COPYDIR/.falsify-sga-11.py"
    chmod +x "$COPYDIR/.falsify-sga-11.py"
    run python3 "$COPYDIR/.falsify-sga-11.py" --gates-only
    if [ "$RC" -ne 0 ] && grep -q 'GATE APPEARED' <<<"$OUT"; then
        ok "11 a coverage verb pointed at $m is GATE APPEARED"
    else
        bad "11 a creative gate spelled as in $m was NOT caught (rc=$RC)"
    fi
done

# --- 12: a needle that has stopped detecting ----------------------------
# Every "no gate here" row is negative against these needles. One that
# silently stopped matching would make such a row read green forever, so the
# witnesses are checked before the rows that depend on them.
sed 's|    "preventsBlockDrops":|    "preventsBlockDropsGONE":|' \
    "$TOOL" > "$COPYDIR/.falsify-sga-12.py"
chmod +x "$COPYDIR/.falsify-sga-12.py"
run python3 "$COPYDIR/.falsify-sga-12.py" --gates-only
if [ "$RC" -ne 0 ] && grep -q 'NEEDLE DEAD' <<<"$OUT"; then
    ok "12 a needle that no longer matches its own witness is NEEDLE DEAD"
else
    bad "12 a dead needle was not caught (rc=$RC)"
fi

# --- 13: GameType must NOT be a needle ----------------------------------
# useItem's spectator gate reads GameType. Adding it to the set would mark
# use_item substrate-blocked, which is the false positive the omission
# prevents — so this asserts the omission still holds.
sed 's|    "isCreative":|    "GameType":|' "$TOOL" > "$COPYDIR/.falsify-sga-13.py"
chmod +x "$COPYDIR/.falsify-sga-13.py"
run python3 "$COPYDIR/.falsify-sga-13.py" --gates-only
if grep -q 'GATE APPEARED' <<<"$OUT"; then
    ok "13 adding GameType to the needle set DOES false-positive use_item —
        the omission is load-bearing, not stylistic"
else
    bad "13 GameType in the needle set did not false-positive; the comment
        explaining its omission is now wrong"
fi

printf '\nsubstrate-gate-audit-falsify: %d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
