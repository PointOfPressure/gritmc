#!/usr/bin/env bash
# entity-broadcast-order-falsify.sh — show ci/entity-broadcast-order.py
# failing before believing it.
#
# This check's headline is "8 of 10 differ, exactly as pinned", and the
# dangerous failure is the opposite of the usual one: a dead reader here
# produces a table that LOOKS like a finding. So the arms below kill each
# reader in turn and demand a refusal, move a real fact and demand a red,
# and — the arm that matters most — re-measure the golden table from a cold
# probe run and require it to agree with the pin figure for figure, because
# a warm pin cannot answer for a probe that has stopped working.
#
# TWO ARMS CHANGED WHEN ROADMAP §103 CLOSED THE DIVERGENCE. Arm 5 used to
# change ours TO vanilla's order and demand a red, so a FIX could not land
# unnoticed. That fix has landed, so the arm would now be asserting that
# nothing changes when nothing changes — a green for free. It is inverted
# rather than deleted: it moves ours AWAY from vanilla's order and demands the
# same red. Arm 5b is new and covers the hole inversion opened — our column is
# now MEASURED by running a dump, so a dump that prints something our code did
# not produce is a way for the check to be wrong that did not exist while the
# column was computed in Python.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
CHK=ci/entity-broadcast-order.py
PIN=ci/checks/entity-broadcast-order.json
OURS=crates/rustmc-server/src/entities.rs
TMP="$(mktemp -d)"
TRK=crates/rustmc-server/src/tracker.rs
MAP=crates/rustmc-server/src/openhash.rs
export PATH="$HOME/.cargo/bin:$PATH"
cp "$CHK" "$TMP/chk.py"; cp "$PIN" "$TMP/pin.json"; cp "$OURS" "$TMP/ours.rs"; cp "$TRK" "$TMP/trk.rs"; cp "$MAP" "$TMP/map.rs"
restore() { cp "$TMP/chk.py" "$CHK"; cp "$TMP/pin.json" "$PIN"; cp "$TMP/ours.rs" "$OURS"; cp "$TMP/trk.rs" "$TRK"; cp "$TMP/map.rs" "$MAP"; }
trap 'restore; rm -rf "$TMP"' EXIT

fail=0
arm() { printf '  %-62s %s\n' "$1" "$2"; }
expect_refusal() {
    out="$(python3 "$CHK" --check 2>/dev/null)"; rc=$?
    if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "$1" "REFUSED (3), stdout empty  OK"
    else arm "$1" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi
    restore
}

# 1. THE COLD RE-DERIVATION. The golden table is a cache and a cache is how a
#    dead probe keeps answering. Re-measure into a scratch copy and diff.
cp "$PIN" "$TMP/before.json"
python3 "$CHK" --measure > /dev/null 2>&1
if diff -q "$TMP/before.json" "$PIN" > /dev/null 2>&1; then
    arm "cold re-measure of the golden table against the pin" "identical  OK"
else arm "cold re-measure of the golden table against the pin" "DIFFERS  FAIL"; fail=1
     diff "$TMP/before.json" "$PIN" | head -20; fi
restore

# 2. the vanilla-side premise moved: ChunkMap.tick no longer walks the map.
python3 - "$CHK" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
a = '"ObjectCollection.iterator" in body'
assert a in s, "arm 2 lost its anchor"
open(p, "w").write(s.replace(a, '"ThisStringIsNotInAnyBytecode" in body'))
EOP
expect_refusal "ChunkMap.tick no longer walks entityMap into sendChanges"

# 3. our reader dies: the container is renamed out from under it.
sed -i 's/by_id: IntOpenHashMap<Entity>,/by_id: IntOpenHashMapRenamed<Entity>,/' "$OURS"
expect_refusal "our entity container renamed away"

# 3b. our CONSUMER reader dies. Grading Entities::all says nothing if the
#     broadcast loop stopped calling it, which is this branch's own
#     failure mode pointed at itself.
sed -i 's/let live = entities.all();/let live = entities.all_renamed();/' "$TRK"
expect_refusal "the broadcast loop no longer walks Entities::all"

# 3c. THE SECOND WALK. `ChunkMap.tick` walks `entityMap.values()` twice, and
#     ours splits that map across `Entities.by_id` and `Tracker.tracked`. Put
#     the sweep back on an ordered container and the check must refuse: it
#     grades the ids `Entities::all` returns, and the sweep emits from a
#     different one.
sed -i 's/tracked: Mutex<IntOpenHashMap<TrackedEntity>>,/tracked: Mutex<BTreeMap<i32, TrackedEntity>>,/' "$TRK"
expect_refusal "the movedPlayers sweep put back on an ordered map"

# 4. the golden was measured against a different fastutil.
python3 - "$PIN" <<'EOP'
import json, sys
p = sys.argv[1]; d = json.load(open(p))
d["fastutil_sha256"] = "0" * 64
json.dump(d, open(p, "w"), indent=2, sort_keys=True)
EOP
expect_refusal "golden table stamped with a different fastutil sha256"

# 5. A MOVE IN OUR ORDER MUST BE NOTICED, AND IT NOW POINTS THE OTHER WAY.
#
#    This arm used to change ours TO vanilla's order and demand a red, so the
#    check would notice a FIX and not only a regression. Roadmap §103 landed
#    that fix: the pin now records 0 divergent sets, and "ours changed to
#    vanilla's order" is no longer a change at all — the arm would have gone
#    quietly green for ever, which is the exact rot it was written to prevent.
#
#    So it is inverted rather than deleted. It moves ours AWAY from vanilla's
#    order — the descending scan in `openhash.rs::walk_slots` flipped to
#    ascending, which is precisely the divergence §101.5 measured and §103
#    closed — and requires the same red. Same arm, same guarantee, opposite
#    direction, and it is why this file rebuilds the crate: nothing cheaper
#    can move a walk that is measured by running it.
python3 - "$MAP" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
a = """        let mut pos = self.n;
        while pos > 0 {
            pos -= 1;
            if self.key[pos] != 0 {
                out.push(pos);
            }
        }"""
assert a in s, "arm 5 lost its anchor"
b = """        for pos in 0..self.n {
            if self.key[pos] != 0 {
                out.push(pos);
            }
        }"""
open(p, "w").write(s.replace(a, b))
EOP
out="$(python3 "$CHK" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 1 ]; then arm "ours moved away from vanilla's order, pin now stale" "RED (1)  OK"
else arm "ours moved away from vanilla's order, pin now stale" "rc=$rc  FAIL"; fail=1; fi
restore

# 5b. AND THE READER MUST NOT BE ABLE TO LIE FOR IT. The dump is the only
#     thing that speaks for our column, so a dump that prints a walk our code
#     did not produce must also be caught. Without this arm 5 proves only that
#     SOMETHING moved, not that the check is reading the real container.
python3 - "$OURS" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
a = 'let walk: Vec<String> = es.all().iter().map(|e| e.id.to_string()).collect();'
assert a in s, "arm 5b lost its anchor"
b = ('let mut sorted_ids = ids.clone();\n'
     '            sorted_ids.sort_unstable();\n'
     '            let walk: Vec<String> = sorted_ids.iter().map(std::string::ToString::to_string).collect();')
open(p, "w").write(s.replace(a, b))
EOP
out="$(python3 "$CHK" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 1 ]; then arm "the dump prints ascending ids instead of the walk" "RED (1)  OK"
else arm "the dump prints ascending ids instead of the walk" "rc=$rc  FAIL"; fail=1; fi
restore

# 6. the jar reader dies.
BAD="$TMP/notajar.jar"; printf 'not a zip' > "$BAD"
out="$(RUSTMC_JAR="$BAD" python3 "$CHK" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "javap pointed at a file that is not a jar" "REFUSED (3), stdout empty  OK"
else arm "javap pointed at a file that is not a jar" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi

# 7. NEGATIVE CONTROL: an unrelated comment in the source we read.
printf '\n// entity-broadcast-order-falsify.sh negative control; removed on exit.\n' >> "$OURS"
out="$(python3 "$CHK" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 0 ]; then arm "NEGATIVE CONTROL: unrelated comment appended" "green (0)  OK"
else arm "NEGATIVE CONTROL: unrelated comment appended" "rc=$rc  FAIL"; fail=1; fi
restore

# 8. NEGATIVE CONTROL: untouched tree.
python3 "$CHK" --check > /dev/null 2>&1; rc=$?
if [ "$rc" -eq 0 ]; then arm "NEGATIVE CONTROL: untouched tree" "green (0)  OK"
else arm "NEGATIVE CONTROL: untouched tree" "rc=$rc  FAIL"; fail=1; fi

if [ "$fail" -ne 0 ]; then echo "entity-broadcast-order-falsify: FAILED"; exit 1; fi
echo "entity-broadcast-order-falsify: 1 cold re-derivation, 8 injected faults caught, 2 negative controls green"
