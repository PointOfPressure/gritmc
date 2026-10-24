#!/usr/bin/env bash
# fastutil-open-hash-walk-falsify.sh — show ci/fastutil-open-hash-walk.py, and
# the replay test that stands on its golden, failing before believing either.
#
# This pair's headline is "10 inputs and 2 consumers, exactly as pinned" plus
# "948 histories replayed". Both are SAMENESS readings, which a dead instrument
# produces for free, and one of them is a data file that a truncation would
# quietly weaken rather than break. So:
#
#   * each reader is blinded in turn and must REFUSE with an empty stdout;
#   * the golden is re-measured from a cold probe run and must agree line for
#     line, because a committed table is how a dead probe keeps answering;
#   * the golden is weakened three ways — truncated, stripped of removes,
#     stripped of drains — and each must be refused, because a replay over a
#     corpus that covers nothing passes for free;
#   * and the PORT itself is broken, which must turn the Rust replay test red.
#     That last arm is the one that decides whether any of this is load
#     bearing: a golden nothing would notice being wrong about is a data file.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
export PATH="$HOME/.cargo/bin:$PATH"
CHK=ci/fastutil-open-hash-walk.py
PIN=ci/checks/fastutil-open-hash-walk.json
GOLD=ci/checks/fastutil-walk-history-golden.txt
OURS=crates/rustmc-server/src/openhash.rs
TMP="$(mktemp -d)"
cp "$CHK" "$TMP/chk.py"; cp "$PIN" "$TMP/pin.json"; cp "$GOLD" "$TMP/gold.txt"; cp "$OURS" "$TMP/ours.rs"
restore() { cp "$TMP/chk.py" "$CHK"; cp "$TMP/pin.json" "$PIN"; cp "$TMP/gold.txt" "$GOLD"; cp "$TMP/ours.rs" "$OURS"; }
trap 'restore; rm -rf "$TMP"' EXIT

fail=0
arm() { printf '  %-64s %s\n' "$1" "$2"; }
expect_refusal() {
    out="$(python3 "$CHK" --check 2>/dev/null)"; rc=$?
    if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "$1" "REFUSED (3), stdout empty  OK"
    else arm "$1" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi
    restore
}

# 1. THE COLD RE-DERIVATION, both artefacts. A pin and a golden are caches.
cp "$PIN" "$TMP/before.json"; cp "$GOLD" "$TMP/before.txt"
python3 "$CHK" --measure > /dev/null 2>&1
if diff -q "$TMP/before.json" "$PIN" > /dev/null 2>&1 && diff -q "$TMP/before.txt" "$GOLD" > /dev/null 2>&1; then
    arm "cold re-measure of the ten inputs and 948 walks" "identical  OK"
else arm "cold re-measure of the ten inputs and 948 walks" "DIFFERS  FAIL"; fail=1
     diff "$TMP/before.txt" "$GOLD" | head -10; fi
restore

# 2. the mix reader goes blind — the constant it hunts is renamed away.
python3 - "$CHK" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
a = 'b = body(dump, "public static int mix(int);", "HashCommon.mix(int)")'
assert a in s, "arm 2 lost its anchor"
open(p, "w").write(s.replace(a, 'b = body(dump, "public static int mixRenamed(int);", "HashCommon.mix(int)")'))
EOP
expect_refusal "the mix reader can no longer find mix()"

# 3. THE DIRECTION READER. The single fact nothing else in the port implies.
python3 - "$CHK" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
a = 'nxt = body(dump_iter, "public int nextEntry();", "MapIterator.nextEntry")'
assert a in s, "arm 3 lost its anchor"
open(p, "w").write(s.replace(a, 'nxt = body(dump_iter, "public int nextEntryRenamed();", "MapIterator.nextEntry")'))
EOP
expect_refusal "the walk-direction reader can no longer find nextEntry()"

# 4. the fastutil reader dies: pointed at a file that is not a jar.
BAD="$TMP/notajar.jar"; printf 'not a zip' > "$BAD"
out="$(RUSTMC_FASTUTIL="$BAD" python3 "$CHK" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "javap pointed at a file that is not a jar" "REFUSED (3), stdout empty  OK"
else arm "javap pointed at a file that is not a jar" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi
restore

# 5. the golden stamped with a fastutil nobody measured it against.
sed -i 's/^# fastutil_sha256: .*/# fastutil_sha256: 0000000000000000000000000000000000000000000000000000000000000000/' "$GOLD"
expect_refusal "the walk golden stamped with a different fastutil sha256"

# 6. ANTI-VACUITY: the golden truncated. A replay over 40 histories passes.
head -20 "$TMP/gold.txt" > "$GOLD"
expect_refusal "the walk golden truncated to its header and a few rows"

# 7. ANTI-VACUITY: every remove stripped, so the shrink and shift-back rules
#    are never exercised and an insert-only port would replay green.
python3 - "$GOLD" "$TMP/gold.txt" <<'EOP'
import sys
out = []
for line in open(sys.argv[2], encoding="utf-8"):
    if line.startswith("#") or not line.strip():
        out.append(line); continue
    ops = line.rstrip("\n").split("|")[0]
    if any(o[0] == "-" for o in ops.split()):
        continue
    out.append(line)
open(sys.argv[1], "w", encoding="utf-8").writelines(out)
EOP
expect_refusal "every history containing a remove deleted from the golden"

# 8. ANTI-VACUITY: the walks replaced by their own keys in ascending order,
#    which is what a probe that returned its input unchanged would produce.
python3 - "$GOLD" "$TMP/gold.txt" <<'EOP'
import sys
out = []
for line in open(sys.argv[2], encoding="utf-8"):
    if line.startswith("#") or not line.strip():
        out.append(line); continue
    ops, walk, obj = line.rstrip("\n").split("|")
    ks = " ".join(str(k) for k in sorted(int(x) for x in walk.split())) if walk.strip() else ""
    # Both columns, so this arm is refused for being ascending rather than for
    # the two containers disagreeing — a different refusal than the one it is
    # written to demonstrate.
    out.append("%s|%s|%s\n" % (ops, ks, ks))
open(sys.argv[1], "w", encoding="utf-8").writelines(out)
EOP
expect_refusal "every golden walk replaced by its keys in ascending order"

# 8b. THE CLAIM THAT ONE WALK COVERS TWO SITES. `PlayerMap.players` is an
#     Object2BooleanOpenHashMap and `ChunkMap.entityMap` an
#     Int2ObjectOpenHashMap; §101 concluded they walk the same order because
#     `Entity.hashCode()` returns `getId()`. That is two generated classes
#     agreeing, not a theorem, so it is measured per row. Break one row's
#     object column and the check must refuse rather than pin a walk that is
#     right for one of the two sites.
python3 - "$GOLD" <<'EOP'
import sys
p = sys.argv[1]
lines = open(p, encoding="utf-8").read().splitlines(keepends=True)
for i, line in enumerate(lines):
    if line.startswith("#") or not line.strip():
        continue
    ops, walk, obj = line.rstrip("\n").split("|")
    if len(walk.split()) >= 2:
        lines[i] = "%s|%s|%s\n" % (ops, walk, " ".join(reversed(walk.split())))
        break
else:
    raise SystemExit("arm 8b found no row with two keys")
open(p, "w", encoding="utf-8").writelines(lines)
EOP
expect_refusal "one golden row's object-keyed walk disagrees with the int one"

# 9. our port stops naming one of the ten inputs, so a parameter that moved in
#    the jar would have nowhere in our source to be wrong.
sed -i "s/8_shrink/8_shrunk/g" "$OURS"
expect_refusal "our port no longer cites the shrink rule it reproduces"

# 10. THE ARM THAT DECIDES WHETHER ANY OF THIS IS LOAD BEARING. Break the walk
#     — ascending instead of descending, which is the divergence this whole
#     lane exists to close — and the replay test must go RED. A golden nothing
#     would notice being wrong about is a data file, not a check.
python3 - "$OURS" <<'EOP'
import sys
p = sys.argv[1]; s = open(p).read()
a = """        let mut pos = self.n;
        while pos > 0 {
            pos -= 1;
            if self.key[pos] != 0 {
                out.push(pos);
            }
        }
        out"""
assert a in s, "arm 10 lost its anchor"
b = """        for pos in 0..self.n {
            if self.key[pos] != 0 {
                out.push(pos);
            }
        }
        out"""
open(p, "w").write(s.replace(a, b))
EOP
if cargo test -q -p rustmc-server --lib openhash::tests::agrees_with_the_measured_vanilla_walks > "$TMP/t.log" 2>&1; then
    arm "the walk flipped to ascending, replay test" "PASSED — the golden is decorative  FAIL"; fail=1
else
    arm "the walk flipped to ascending, replay test" "RED  OK"
fi

# 10b. AND THE SAME MUTATION MUST REACH THE WIRE. Everything else in this file
#      grades a container. `three_entities_moving_in_one_tick_...` reads the
#      frames a real Connection received, which is where the parity claim
#      actually lives: a walk that is right in `Entities::all` and re-sorted
#      anywhere between there and `send` would pass every other arm here.
if cargo test -q -p rustmc-server --lib tracker::tests::three_entities_moving_in_one_tick_leave_in_vanillas_broadcast_order > "$TMP/t2.log" 2>&1; then
    arm "the walk flipped to ascending, the wire-order test" "PASSED  FAIL"; fail=1
else
    arm "the walk flipped to ascending, the wire-order test" "RED  OK"
fi
restore

# 11. NEGATIVE CONTROL: an unrelated comment in the source the check reads.
printf '\n// fastutil-open-hash-walk-falsify.sh negative control; removed on exit.\n' >> "$OURS"
out="$(python3 "$CHK" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 0 ]; then arm "NEGATIVE CONTROL: unrelated comment appended" "green (0)  OK"
else arm "NEGATIVE CONTROL: unrelated comment appended" "rc=$rc  FAIL"; fail=1; fi
restore

# 12. NEGATIVE CONTROL: untouched tree, both instruments.
python3 "$CHK" --check > /dev/null 2>&1; rc=$?
if [ "$rc" -eq 0 ]; then arm "NEGATIVE CONTROL: untouched tree, the check" "green (0)  OK"
else arm "NEGATIVE CONTROL: untouched tree, the check" "rc=$rc  FAIL"; fail=1; fi

if cargo test -q -p rustmc-server --lib openhash::tests::agrees_with_the_measured_vanilla_walks > /dev/null 2>&1; then
    arm "NEGATIVE CONTROL: untouched tree, the replay test" "green  OK"
else arm "NEGATIVE CONTROL: untouched tree, the replay test" "RED  FAIL"; fail=1; fi

if [ "$fail" -ne 0 ]; then echo "fastutil-open-hash-walk-falsify: FAILED"; exit 1; fi
echo "fastutil-open-hash-walk-falsify: 1 cold re-derivation, 11 injected faults caught, 3 negative controls green"
