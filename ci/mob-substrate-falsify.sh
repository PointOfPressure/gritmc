#!/usr/bin/env bash
# mob-substrate-falsify.sh — inject faults, assert ci/mob-substrate.py catches each.
#
# This gate's whole job is to notice when the world the corpus boots changes,
# because every mob finding on this branch is conditional on that world. The
# failure that would make it worthless is a parser that reads nothing: it
# would report a world with NO suppressions, which is the most misleading
# output the file could produce — it would say the corpus can measure
# spawning. So the reader's liveness is asserted here, not assumed.
#
# Six assertions, all in a scratch tree:
#   1  the clean copy is green
#   2  changing difficulty=peaceful to hard is refused, and named
#   3  dropping a game rule is refused
#   4  an unrelated edit to ci/t2-gameplay.sh is NOT refused (negative control)
#   5  blanking VANILLA_EXTRA_PROPS fires the reader control: exit 3, nothing
#      on stdout
#   6  a jar read that returns nothing fires it too — the other input
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
FAIL=0
note() { printf '  %-5s %s\n' "$1" "$2"; }
ok()   { note "ok" "$1"; }
bad()  { note "FAIL" "$1"; FAIL=1; }

# THE SUBSTRATE IS THREE FILES SINCE 2026-09-05, NOT ONE. The four game rules
# moved to `ci/gameplay-substrate.sh` and the difficulty to
# `ci/gameplay-difficulty.sh`, both sourced by `ci/t2-gameplay.sh`, and this
# fixture copied only the gate — so the reader found nothing, refused, and case
# 1 went red before any fault was injected. That is the fixture being stale,
# not the reader being wrong, and it is why case 1 exists.
mkdir -p "$SCRATCH/ci/checks"
cp "$HERE/mob-substrate.py" "$SCRATCH/ci/"
cp "$HERE/t2-gameplay.sh" "$SCRATCH/ci/"
cp "$HERE/gameplay-substrate.sh" "$SCRATCH/ci/"
cp "$HERE/gameplay-difficulty.sh" "$SCRATCH/ci/"
cp "$HERE/checks/mob-substrate.json" "$SCRATCH/ci/checks/"
SUB="$SCRATCH/ci/mob-substrate.py"
T2="$SCRATCH/ci/t2-gameplay.sh"
GS="$SCRATCH/ci/gameplay-substrate.sh"
GD="$SCRATCH/ci/gameplay-difficulty.sh"
cp "$T2" "$SCRATCH/t2.orig"
cp "$GS" "$SCRATCH/gs.orig"
cp "$GD" "$SCRATCH/gd.orig"
cp "$SUB" "$SCRATCH/sub.orig"
restore() { cp "$SCRATCH/t2.orig" "$T2"; cp "$SCRATCH/gs.orig" "$GS"; cp "$SCRATCH/gd.orig" "$GD"; }

echo "== 1: the clean copy is green"
if python3 "$SUB" --check >/dev/null 2>&1; then ok "--check green"
else bad "--check red before any fault — every later assertion is vacuous"; fi

echo "== 2: moving the difficulty is refused"
# The value now lives in `ci/gameplay-difficulty.sh`'s default rather than as a
# literal in the gate, which is exactly the kind of move this case has to keep
# following: the substrate is what the harness RESOLVES to, not where it is
# typed.
sed -i 's/GAMEPLAY_DIFFICULTY:-peaceful/GAMEPLAY_DIFFICULTY:-hard/' "$GD"
ERR="$(python3 "$SUB" --check 2>&1 >/dev/null)"; RC=$?
if [ $RC -ne 0 ]; then ok "refused"; else bad "a changed difficulty went unrefused"; fi
if printf '%s' "$ERR" | grep -c "properties" >/dev/null; then ok "named the field"; else bad "refused without naming it"; fi
restore

echo "== 3: dropping a game rule is refused"
sed -i 's/SUBSTRATE_GAMERULES=(advance_time=false spawn_mobs=false/SUBSTRATE_GAMERULES=(advance_time=false/' "$GS"
if python3 "$SUB" --check >/dev/null 2>&1; then
    bad "a dropped game rule went unrefused"
else
    ok "refused"
fi
restore

echo "== 3b: the substrate moving OUT of a sourced file is refused, not reported empty"
#
# THE 2026-09-05 RED, AS A CASE. The rules moved from `ci/t2-gameplay.sh` into
# a file it sources, and a reader that only read the gate saw a corpus with no
# suppressions — the most misleading output this script can produce, and the
# same unfalsifiable shape twice repaired in the harness that night. Deleting
# the source line is the mechanised version of that move.
grep -v 'ci/gameplay-substrate.sh"' "$T2" > "$T2.new" && mv "$T2.new" "$T2"
OUT="$(python3 "$SUB" 2>/dev/null)"; RC=$?
ERR="$(python3 "$SUB" 2>&1 >/dev/null)"
if [ $RC -eq 3 ] && [ -z "$OUT" ]; then ok "exit 3 and nothing printed"
else bad "a substrate the gate no longer sources was reported as an empty one (rc=$RC)"; fi
if printf '%s' "$ERR" | grep -c "read 0 game rules" >/dev/null; then ok "named the empty reading"
else bad "refused without saying what it failed to read"; fi
restore

echo "== 3c: a setting whose value cannot be resolved is refused"
#
# `difficulty=$GAMEPLAY_DIFFICULTY` with no default anywhere is a setting whose
# VALUE nobody has established, and reporting it as a suppression is a claim
# about a world that was never booted. Dropping the default line is how that
# arises in practice.
grep -v 'GAMEPLAY_DIFFICULTY:-peaceful' "$GD" > "$GD.new" && mv "$GD.new" "$GD"
ERR="$(python3 "$SUB" --check 2>&1 >/dev/null)"; RC=$?
if [ "$RC" -ne 0 ]; then ok "refused"; else bad "an unresolved substrate parameter went unrefused"; fi
if printf '%s' "$ERR" | grep -c 'could not resolve' >/dev/null; then ok "named the parameter"
else bad "refused without naming the parameter"; fi
restore

echo "== 4: an unrelated edit is not refused (negative control)"
printf '\n# falsify: a comment that changes no substrate\n' >> "$T2"
if python3 "$SUB" --check >/dev/null 2>&1; then
    ok "not refused — the check reads the substrate, not the file"
else
    bad "refused on a comment; it is hashing the file and assertions 2 and 3 prove nothing"
fi
restore

echo "== 5: blanking the properties fires the reader control"
# Both spellings, because the block stopped being a `$'…'` ANSI-C string on
# 2026-09-05 — that form does not expand parameters and the substrate now names
# one. A sed that only knew the old form silently injected nothing and the case
# passed on an unblanked file.
sed -i "s/VANILLA_EXTRA_PROPS=\$'[^']*'/VANILLA_EXTRA_PROPS=\$''/" "$T2"
python3 - "$T2" <<'PY'
import re, sys
p = sys.argv[1]; s = open(p).read()
s = re.sub(r'VANILLA_EXTRA_PROPS="[^"]*"', 'VANILLA_EXTRA_PROPS=""', s)
open(p, "w").write(s)
PY
OUT="$(python3 "$SUB" 2>/dev/null)"; RC=$?
ERR="$(python3 "$SUB" 2>&1 >/dev/null)"
if [ $RC -eq 3 ]; then ok "exit 3"; else bad "expected exit 3, got $RC"; fi
if [ -z "$OUT" ]; then ok "printed nothing"; else bad "printed a substrate report anyway"; fi
if printf '%s' "$ERR" | grep -c "not live" >/dev/null; then ok "said the reader is not live"; else bad "refusal was unexplained"; fi
restore

echo "== 6: a jar read that returns nothing fires it too"
python3 - "$SUB" <<'PY'
import sys
p=sys.argv[1]; s=open(p).read()
s=s.replace("def jar_types():", "def jar_types():\n    return []  # falsify: the jar reader is dead\n\ndef _jar_types_real():")
open(p,"w").write(s)
PY
OUT="$(python3 "$SUB" 2>/dev/null)"; RC=$?
ERR="$(python3 "$SUB" 2>&1 >/dev/null)"
if [ $RC -eq 3 ] && [ -z "$OUT" ]; then
    ok "exit 3 and nothing printed"
else
    bad "a dead jar reader still produced a report (rc=$RC)"
fi
if printf '%s' "$ERR" | grep -c "notInPeaceful" >/dev/null; then
    ok "named the impossible reading"
else
    bad "refused without saying what was impossible"
fi
cp "$SCRATCH/sub.orig" "$SUB"

echo
if [ $FAIL -eq 0 ]; then
    echo "mob-substrate-falsify: all assertions held"
else
    echo "mob-substrate-falsify: RED — an injected fault went unrefused" >&2
fi
exit $FAIL
