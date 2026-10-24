#!/usr/bin/env bash
# order-keyed-census-falsify.sh — show ci/order-keyed-census.py failing first.
#
# The census's headline is a TABLE, and a table is the reading a broken parser
# produces most convincingly: eleven rows all saying SORTED is exactly what a
# reader that found nothing and defaulted would print. So every positive arm
# breaks a different reader and demands a refusal with an empty stdout.
#
# Two negative controls, because the arms above would all pass for a check
# that refused on any input at all.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
SRC=ci/order-keyed-census.py
TMP="$(mktemp -d)"
cp "$SRC" "$TMP/orig.py"
restore() { cp "$TMP/orig.py" "$SRC"; }
trap 'restore; rm -rf "$TMP"' EXIT

fail=0
arm() { printf '  %-62s %s\n' "$1" "$2"; }
expect_refusal() {
    out="$(python3 "$SRC" --check 2>/dev/null)"; rc=$?
    if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "$1" "REFUSED (3), stdout empty  OK"
    else arm "$1" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi
    restore
}

# 1. the container reader dies: the `new` no longer matches.
sed -i 's|r"new\\s+#\\d+\\s+// class (\[a-zA-Z0-9/\$\]+)"|r"nevermatchesanything ([a-z]+)"|' "$SRC"
expect_refusal "the bytecode container reader can no longer see a construction"

# 2. an unknown container must refuse rather than default. Drop the entry the
#    site that reaches the most packets depends on.
sed -i '/"it\/unimi\/dsi\/fastutil\/ints\/Int2ObjectOpenHashMap": "KEYHASH",/d' "$SRC"
expect_refusal "a container with no declared order class"

# 3. an OBJHASH site with no declared key type cannot be classified, so the
#    census must refuse rather than call it OBJHASH and move on.
sed -i 's|keytype="net.minecraft.world.entity.ai.attributes.AttributeInstance",|keytype=None,|' "$SRC"
expect_refusal "a hash-over-objects site with no key type to test hashCode on"

# 4. the hashCode reader dies, so IDENTITY and OBJHASH stop being separable.
#    Forcing it to report an override turns three IDENTITY rows into
#    OBJHASH, which is a pinned figure moving: red, not refusal.
out="$(RUSTMC_ORDER_FORCE_HASHCODE=1 python3 "$SRC" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 1 ]; then arm "hashCode reader always says 'overridden'" "RED (1)  OK"
else arm "hashCode reader always says 'overridden'" "rc=$rc  FAIL"; fail=1; fi

# 5. the jar reader dies.
BAD="$TMP/notajar.jar"; printf 'not a zip' > "$BAD"
out="$(RUSTMC_JAR="$BAD" python3 "$SRC" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "javap pointed at a file that is not a jar" "REFUSED (3), stdout empty  OK"
else arm "javap pointed at a file that is not a jar" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi

# 6. NEGATIVE CONTROL: a comment appended to the census itself changes nothing.
printf '\n# order-keyed-census-falsify.sh negative control; removed on exit.\n' >> "$SRC"
out="$(python3 "$SRC" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 0 ]; then arm "NEGATIVE CONTROL: unrelated comment appended" "green (0)  OK"
else arm "NEGATIVE CONTROL: unrelated comment appended" "rc=$rc  FAIL"; fail=1; fi
restore

# 7. NEGATIVE CONTROL: untouched tree is green.
python3 "$SRC" --check > /dev/null 2>&1; rc=$?
if [ "$rc" -eq 0 ]; then arm "NEGATIVE CONTROL: untouched tree" "green (0)  OK"
else arm "NEGATIVE CONTROL: untouched tree" "rc=$rc  FAIL"; fail=1; fi

if [ "$fail" -ne 0 ]; then echo "order-keyed-census-falsify: FAILED"; exit 1; fi
echo "order-keyed-census-falsify: 5 injected faults refused or reddened, 2 negative controls green"
