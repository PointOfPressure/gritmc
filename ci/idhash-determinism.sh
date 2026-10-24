#!/usr/bin/env bash
# idhash-determinism.sh — is an identity-hash iteration order run-stable?
#
# # Why this is not a curiosity
#
# `ci/order-keyed-census.py` finds three sites on the wire whose order is a
# function of `Object.hashCode()` on types that never override it:
# `AttributeMap.attributes`, `AttributeMap.attributesToSync`, and
# `ServerChunkCache.chunkHoldersToBroadcast`. How to READ those rows depends
# entirely on one question this project had been answering from intuition.
#
# The intuition was "identity hash is random per run, so vanilla disagrees
# with itself and matching it is not a requirement". That is wrong, and the
# two probes here are what makes it wrong rather than an opinion:
#
#   * IdHash  — the same allocation sequence produces the SAME identity
#     hashes on every run of the JVM, in both a single-threaded and a
#     four-thread arm. HotSpot's hash comes from a per-thread xorshift whose
#     seed is a fixed constant, not from entropy.
#   * IdHash2 — the hash is drawn from that per-thread sequence at the moment
#     `hashCode()` is FIRST called, so it is a property of the DRAW ORDER and
#     not of the object. Hash the same eight objects in a different order and
#     each gets a different number.
#
# Which gives the rule the census's IDENTITY rows are read under:
#
#     an identity-hash iteration order is run-stable exactly when the order
#     of first-hash calls into that container is run-stable.
#
# So `broadcastChangedChunks` measuring 4.24% self-agreement against 1/4! is
# NOT evidence that identity hashing is random. It is evidence that which
# chunk is marked dirty first varies between runs. The nondeterminism is
# upstream and the set only exposes it. An attribute map built on the server
# thread in a fixed order has no such upstream, and vanilla agrees with
# itself about it in every run this corpus holds.
#
# # The controls
#
# Positive: a probe that printed nothing, or the same line for every input,
# would "prove" determinism. Arm 3 requires arms A and B to DIFFER — a probe
# that cannot tell two orders apart fails here — and arm 4 requires the
# output to be non-empty and to have the expected shape.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
RUNS="${IDHASH_RUNS:-5}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

command -v javac > /dev/null 2>&1 || { echo "idhash-determinism: REFUSED — no javac" >&2; exit 3; }
javac -d "$TMP" ci/probes/IdHash.java ci/probes/IdHash2.java 2>&1 | head -5
[ -f "$TMP/IdHash.class" ] && [ -f "$TMP/IdHash2.class" ] \
    || { echo "idhash-determinism: REFUSED — probes did not compile" >&2; exit 3; }

fail=0
arm() { printf '  %-56s %s\n' "$1" "$2"; }

# 1 + 2. both arms of IdHash identical across RUNS cold JVMs.
: > "$TMP/out"
for _ in $(seq "$RUNS"); do java -cp "$TMP" IdHash >> "$TMP/out" || exit 3; done
for a in main pool; do
    n=$(grep -c "^$a " "$TMP/out")
    d=$(grep "^$a " "$TMP/out" | sort -u | wc -l)
    if [ "$n" -eq "$RUNS" ] && [ "$d" -eq 1 ]; then
        arm "IdHash arm '$a': $RUNS cold JVMs" "1 distinct  OK"
    else arm "IdHash arm '$a': $RUNS cold JVMs" "$n lines, $d distinct  FAIL"; fail=1; fi
done

# 3. THE DISCRIMINATOR. A and B must differ, or the probe cannot tell a
#    draw-order-dependent hash from an object-dependent one and arms 1-2 are
#    consistent with "every object hashes to the same number".
java -cp "$TMP" IdHash2 > "$TMP/o2" || exit 3
A=$(grep '^A(' "$TMP/o2" | cut -d' ' -f2-)
B=$(grep '^B(scrambled' "$TMP/o2" | cut -d' ' -f2-)
if [ -n "$A" ] && [ "$A" != "$B" ]; then
    arm "IdHash2: hashing eight objects in a different order" "different hashes  OK"
else arm "IdHash2: hashing eight objects in a different order" "A='$A' B='$B'  FAIL"; fail=1; fi

# 4. and IdHash2 is itself run-stable, which is the claim being made.
: > "$TMP/o2all"
for _ in $(seq "$RUNS"); do java -cp "$TMP" IdHash2 >> "$TMP/o2all" || exit 3; done
d=$(sort -u "$TMP/o2all" | wc -l); n=$(wc -l < "$TMP/o2all")
if [ "$n" -eq $((RUNS * 3)) ] && [ "$d" -eq 3 ]; then
    arm "IdHash2: $RUNS cold JVMs" "3 distinct lines, all repeated  OK"
else arm "IdHash2: $RUNS cold JVMs" "$n lines, $d distinct  FAIL"; fail=1; fi

# 5. NEGATIVE CONTROL: the probes produced real output, not silence that the
#    `sort -u | wc -l` arms above would have scored as "1 distinct".
lines=$(wc -l < "$TMP/out")
if [ "$lines" -eq $((RUNS * 2)) ]; then arm "NEGATIVE CONTROL: probe output is non-empty" "$lines lines  OK"
else arm "NEGATIVE CONTROL: probe output is non-empty" "$lines lines  FAIL"; fail=1; fi

if [ "$fail" -ne 0 ]; then echo "idhash-determinism: FAILED"; exit 1; fi
echo "idhash-determinism: identity hashes are run-stable and are drawn in first-hash order ($RUNS runs)"
