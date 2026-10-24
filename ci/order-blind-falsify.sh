#!/usr/bin/env bash
# order-blind-falsify.sh — show ci/order-blind.py failing first.
#
# Its headline is `0 can grade order`, which is a ZERO, which is what a broken
# reader reports for free. Three readers can produce it: the packet reader
# (an empty kind set makes every check blind by projection), the check reader
# (no checks, nothing to grade), and the recording reader (no entities, so
# nobody sees two). Each is killed here and must REFUSE.
#
# The ratchet itself is exercised too, and since roadmap §103 closed the
# divergence there are two of them:
#
#   * the ORIGINAL — a check that can grade the order while ours still differs
#     must go RED. It can no longer fire on the tree as it stands, because
#     nothing differs, so arm 5 reopens the divergence in the pin as well as
#     widening the population test. An arm that cannot fire is a comment, and
#     this is the file that says so about other people's arms;
#   * the CLOSURE FLOOR — the pin recording the order as differing again, with
#     no check able to see it, must also go RED. That is arm 5b, and it is the
#     one that guards the escape hatch: `entity-broadcast-order.py --measure`
#     rewrites the pin, and with 0 of 108 checks able to grade this order a
#     re-measure is the only way the closure could quietly come undone.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
CHK=ci/order-blind.py
BCPIN=ci/checks/entity-broadcast-order.json
TMP="$(mktemp -d)"
POP=ci/checks/order-blind-population.json
cp "$CHK" "$TMP/orig.py"; cp "$BCPIN" "$TMP/bcpin.json"; cp "$POP" "$TMP/pop.json"
restore() { cp "$TMP/orig.py" "$CHK"; cp "$TMP/bcpin.json" "$BCPIN"; cp "$TMP/pop.json" "$POP"; }
trap 'restore; rm -rf "$TMP"' EXIT

fail=0
arm() { printf '  %-60s %s\n' "$1" "$2"; }
expect_refusal() {
    out="$(python3 "$CHK" --check 2>/dev/null)"; rc=$?
    if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "$1" "REFUSED (3), stdout empty  OK"
    else arm "$1" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi
    restore
}

# 1. the packet reader dies. An empty kind set files every check as blind by
#    projection and prints the same zero the truth prints.
python3 - "$CHK" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
a = 'r"// class net/minecraft/network/protocol/game/(Clientbound[A-Za-z]+)",'
assert a in s, "arm 1 lost its anchor"
open(p, "w").write(s.replace(a, 'r"// class never/matches/anything/(Clientbound[A-Za-z]+)",'))
EOP
expect_refusal "the ServerEntity packet reader finds no packets"

# 2. an unmapped broadcast packet must refuse rather than be silently uncounted.
python3 - "$CHK" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
a = '    "ClientboundRotateHeadPacket": ["rotate_head"],\n'
assert a in s, "arm 2 lost its anchor"
open(p, "w").write(s.replace(a, ""))
EOP
expect_refusal "ServerEntity builds a packet PACKET_TO_KIND does not name"

# 3. the check reader dies.
python3 - "$CHK" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
a = "GAMEPLAY = os.path.join(HERE, \"checks\", \"gameplay\")"
assert a in s, "arm 3 lost its anchor"
open(p, "w").write(s.replace(a, "GAMEPLAY = os.path.join(HERE, \"checks\", \"gameplay-that-does-not-exist\")"))
EOP
expect_refusal "no gameplay checks to read"

# 4. the population reader dies: the committed population is gone. It is a
#    tracked file, so its absence is a broken tree and must refuse, never
#    fall back to whatever the box holds.
rm -f "$POP"
expect_refusal "the committed population (order-blind-population.json) is absent"
# 4b. and a gate cannot be pointed at a glob instead of it.
out="$(python3 "$CHK" --recordings "$TMP/none/*.json" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "--check given --recordings" "REFUSED (3), stdout empty  OK"
else arm "--check given --recordings" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi
# 5. THE ORIGINAL RATCHET. A second check becomes gradeable — injected into
#    the committed population, which is now the only thing --check reads —
#    AND the divergence reopens in the pin, because since §103 the tree has
#    nothing for the first half to be true of. That must go RED, and name it.
promoted="$(python3 - "$POP" ci/checks/order-blind.toml <<'EOP'
import json, re, sys
p = sys.argv[1]; d = json.load(open(p))
m = re.search(r'name = "([^"]+)"\nfile = "[^"]+"\nstatus = "BLIND-BY-POPULATION"\nwatched = \[\s*"([^"]+)"', open(sys.argv[2]).read())
assert m, "arm 5 found no BLIND-BY-POPULATION row to promote"
d["recordings"][0]["pop"].setdefault(m.group(1), {})[m.group(2)] = 2
json.dump(d, open(p, "w"))
print(m.group(1))
EOP
)"
python3 - "$BCPIN" <<'EOP'
import json, sys
p = sys.argv[1]; d = json.load(open(p))
d["ours_matches_vanilla"] = False
d["divergent_sets"] = ["1 2"]
json.dump(d, open(p, "w"), indent=2, sort_keys=True)
EOP
# The closure floor (5b) would fire first on the reopened pin and hide this
# refusal behind its own; take the closure out of the ledger for this arm so
# the refusal under test is the only one that can name the promoted check.
cp ci/checks/order-blind.toml "$TMP/ledger5.toml"
sed -i 's/^broadcast_order_closed = true$/broadcast_order_closed = false/' ci/checks/order-blind.toml
err="$(python3 "$CHK" --check 2>&1 >/dev/null)"; rc=$?
if [ "$rc" -eq 1 ] && printf '%s' "$err" | grep -cF "$promoted" >/dev/null; then arm "a check becomes gradeable while the divergence is open" "RED (1), names it  OK"
else arm "a check becomes gradeable while the divergence is open" "rc=$rc  FAIL"; fail=1; fi
cp "$TMP/ledger5.toml" ci/checks/order-blind.toml
restore
# 5r. THE NARROW READING, which now runs at --extract rather than at --check.
#     Two entities in one window but in different ticks are NOT an order; the
#     wide reading this tool first used counted them. A crafted recording must
#     reduce to 1 as written and to 2 with the bucket widened.
mkdir -p "$TMP/narrow/t"
python3 - "$TMP/narrow/t/vanilla.json" <<'EOP'
import json, sys
o = lambda ms, e: {"kind": "update_attributes", "who": "actor", "at_ms": ms,
                   "fields": {"entity": {"$entity": e}}}
json.dump({"scenarios": [{"name": "narrow-probe", "obs": [o(1000, 1), o(1050, 2)]}]}, open(sys.argv[1], "w"))
EOP
reduced() { python3 -c 'import importlib.util,json,sys
spec=importlib.util.spec_from_file_location("m",sys.argv[1]);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
print(m.reduce_recording(json.load(open(sys.argv[2])))["narrow-probe"]["update_attributes"])' "$CHK" "$TMP/narrow/t/vanilla.json"; }
n1=$(reduced)
python3 - "$CHK" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
a = 'byk[(o["kind"], o["who"], o["at_ms"])].add(e["$entity"])'
assert a in s, "arm 5r lost its anchor"
open(p, "w").write(s.replace(a, 'byk[(o["kind"], 0, 0)].add(e["$entity"])'))
EOP
n2=$(reduced)
if [ "$n1" = 1 ] && [ "$n2" = 2 ]; then arm "two entities in one window, different ticks: narrow 1, wide 2" "OK"
else arm "two entities in one window, different ticks: narrow 1, wide 2" "narrow=$n1 wide=$n2  FAIL"; fail=1; fi
restore
# 5b. THE CLOSURE FLOOR, on its own. No check can see the order — the tree's
#     real state, 0 of 108 — and the pin says ours differs again. Nothing else
#     in the gate reddens on that, which is exactly why this does.
python3 - "$BCPIN" <<'EOP'
import json, sys
p = sys.argv[1]; d = json.load(open(p))
d["ours_matches_vanilla"] = False
d["divergent_sets"] = ["1 2"]
json.dump(d, open(p, "w"), indent=2, sort_keys=True)
EOP
# Matched on the closure refusal's own words, not on rc alone: since 082's
# re-rig (train-a, 2026-09-30) one check CAN grade order, so the second
# refusal also fires on an open divergence and an rc of 1 no longer says
# which arm produced it.
err="$(python3 "$CHK" --check 2>&1 >/dev/null)"; rc=$?
if [ "$rc" -eq 1 ] && printf '%s' "$err" | grep -c 'NOTHING IN THE CORPUS CAN SEE' >/dev/null; then arm "the closed divergence reopens in the pin, nothing can see it" "RED (1)  OK"
else arm "the closed divergence reopens in the pin, nothing can see it" "rc=$rc  FAIL"; fail=1; fi
restore

# 5c. AND THE FLOOR MUST BE THE LEDGER'S, NOT A CONSTANT. Take the closure out
#     of the ledger and the refusal must stop firing — otherwise 5b would pass
#     for any tree at all and would be testing a hard-coded `false`.
cp ci/checks/order-blind.toml "$TMP/ledger.toml"
sed -i 's/^broadcast_order_closed = true$/broadcast_order_closed = false/' ci/checks/order-blind.toml
python3 - "$BCPIN" <<'EOP'
import json, sys
p = sys.argv[1]; d = json.load(open(p))
d["ours_matches_vanilla"] = False
d["divergent_sets"] = ["1 2"]
json.dump(d, open(p, "w"), indent=2, sort_keys=True)
EOP
# The control is that the CLOSURE refusal stops firing. It asserted rc 0
# while no check could grade order; with 082 able to (see 5b), the
# can-grade refusal legitimately fires here on the reopened pin, so the
# assertion is the absence of the closure refusal's words, plus an rc that
# is 0 exactly when nothing can grade order.
err="$(python3 "$CHK" --check 2>&1 >/dev/null)"; rc=$?
can_n="$(grep -c '^status = "CAN-GRADE-ORDER"' "$TMP/ledger.toml")"
want=0; [ "$can_n" -gt 0 ] && want=1
if [ "$rc" -eq "$want" ] && ! printf '%s' "$err" | grep -c 'NOTHING IN THE CORPUS CAN SEE' >/dev/null; then arm "NEGATIVE CONTROL: the ledger does not claim it was closed" "green (0)  OK"
else arm "NEGATIVE CONTROL: the ledger does not claim it was closed" "rc=$rc  FAIL"; fail=1; fi
cp "$TMP/ledger.toml" ci/checks/order-blind.toml
restore

# 6. NEGATIVE CONTROL: a comment appended changes nothing.
printf '\n# order-blind-falsify.sh negative control; removed on exit.\n' >> "$CHK"
out="$(python3 "$CHK" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 0 ]; then arm "NEGATIVE CONTROL: unrelated comment appended" "green (0)  OK"
else arm "NEGATIVE CONTROL: unrelated comment appended" "rc=$rc  FAIL"; fail=1; fi
restore

# 7. NEGATIVE CONTROL: untouched tree.
python3 "$CHK" --check > /dev/null 2>&1; rc=$?
if [ "$rc" -eq 0 ]; then arm "NEGATIVE CONTROL: untouched tree" "green (0)  OK"
else arm "NEGATIVE CONTROL: untouched tree" "rc=$rc  FAIL"; fail=1; fi

# 8. A FOREIGN TREE'S RECORDING CANNOT MOVE --check. The defect of
#    2026-09-30: ~/rustmc-train-c's 15:09 corpus made a check read
#    CAN-GRADE-ORDER in every tree's T0. A scratch copy of the checker whose
#    box glob is a scratch dir holding one crafted recording that promotes a
#    BLIND-BY-POPULATION check: the ad-hoc box reading must list it (the
#    recording is potent), and --check must print byte-for-byte what it prints
#    with that dir empty. Run against a copy whose --check reads the glob, the
#    same arm must come out "moved" — that is what makes it evidence.
foreign_arm() {  # checker-source
    local S="$TMP/f" name kind a b c J
    rm -rf "$S"; mkdir -p "$S/ci/checks" "$S/box/t1"
    cp -r ci/checks/gameplay "$S/ci/checks/"
    cp ci/checks/order-blind.toml "$BCPIN" "$POP" "$S/ci/checks/"
    python3 - "$1" "$S/ci/order-blind.py" "$S/box" <<'EOP'
import sys
s = open(sys.argv[1]).read()
a = 'DEFAULT_GLOB = os.path.expanduser("~/rustmc*/oracle-work/gameplay*/vanilla.json")'
assert a in s, "arm 8 lost its anchor"
open(sys.argv[2], "w").write(s.replace(a, "DEFAULT_GLOB = %r" % (sys.argv[3] + "/*/vanilla.json")))
EOP
    read -r name kind < <(python3 - ci/checks/order-blind.toml <<'EOP'
import re, sys
m = re.search(r'name = "([^"]+)"\nfile = "[^"]+"\nstatus = "BLIND-BY-POPULATION"\nwatched = \[\s*"([^"]+)"', open(sys.argv[1]).read())
print(m.group(1), m.group(2))
EOP
)
    J="${RUSTMC_JAR:-$ROOT/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar}"
    a="$(RUSTMC_JAR="$J" python3 "$S/ci/order-blind.py" --check 2>&1; echo "rc=$?")"
    python3 - "$S/box/t1/vanilla.json" "$name" "$kind" <<'EOP'
import json, sys
o = lambda e: {"kind": sys.argv[3], "who": "actor", "at_ms": 1000, "fields": {"entity": {"$entity": e}}}
json.dump({"scenarios": [{"name": sys.argv[2], "obs": [o(1), o(2)]}]}, open(sys.argv[1], "w"))
EOP
    b="$(RUSTMC_JAR="$J" python3 "$S/ci/order-blind.py" --check 2>&1; echo "rc=$?")"
    c="$(RUSTMC_JAR="$J" python3 "$S/ci/order-blind.py" 2>&1)"
    if ! printf '%s' "$c" | grep -cF "CAN GRADE: ${name:0:40}" >/dev/null; then echo impotent
    elif [ "$a" = "$b" ]; then echo same; else echo moved; fi
}
r=$(foreign_arm "$CHK")
if [ "$r" = same ]; then arm "a foreign recording promoting a check: box reading moves, --check does not" "OK"
else arm "a foreign recording promoting a check: box reading moves, --check does not" "$r  FAIL"; fail=1; fi
python3 - "$CHK" "$TMP/globcheck.py" <<'EOP'
import sys
s = open(sys.argv[1]).read()
a = "    if args.check or args.ledger:\n        pop, N, nfiles = corpus_population()"
b = "    if (args.check or args.ledger) and args.recordings:"
assert a in s and b in s, "arm 8 mutant lost its anchor"
open(sys.argv[2], "w").write(s.replace(a, "    if False:\n        pop, N, nfiles = corpus_population()").replace(b, "    if False:"))
EOP
r=$(foreign_arm "$TMP/globcheck.py")
if [ "$r" = moved ]; then arm "MUTANT: a --check that reads the box is caught by arm 8" "OK"
else arm "MUTANT: a --check that reads the box is caught by arm 8" "$r  FAIL"; fail=1; fi

if [ "$fail" -ne 0 ]; then echo "order-blind-falsify: FAILED"; exit 1; fi

echo "order-blind-falsify: 5 injected faults refused, both ratchets redden, the narrow reading holds, a foreign recording cannot move --check, 3 negative controls green"
