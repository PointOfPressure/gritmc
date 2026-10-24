#!/usr/bin/env bash
# spiral-golden.sh — vanilla's own `BlockPos.spiralAround`, captured.
#
#     ./ci/spiral-golden.sh [out.json]
#
# Produces `crates/rustmc-server/golden/spiral_around.json`, which is a
# **committed fixture compared against**, not a `data/vanilla/` input the
# server reads at runtime. The distinction is the one `ci/attributes-golden.sh`
# documents: a stale input silently changes behaviour, a stale fixture reddens
# a test.
#
# See `tools/goldengen/vanilla/SpiralGolden.java` for which four walks are
# captured and why no single transcription bug passes all of them.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/spiral_around.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }

UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi

CP="$UNSIGNED:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$(dirname "$OUT")"
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/SpiralGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx1G -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.SpiralGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. The iterator is arithmetic over BlockPos and
# Direction and reads no world, no seed and no registry; a disagreement would
# mean the capture is reading something ambient and every position below would
# be a coincidence.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of SpiralGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
walks = d["walks"]
print(f"spiralAround -> {sys.argv[1]} (identical across two runs)")

fail = []

# Derived from the leg rule rather than read off the capture — a validator
# that took its expectation from its input would agree with anything.
#
# `legs = 4 * radius`, and the iterator uses leg values 0..=legs inclusive,
# each contributing `leg // 2 + 1` positions: the end test `leg >= legs` is
# only consulted when a leg RUNS OUT, so the leg numbered `legs` is walked in
# full before the walk stops. Leg 0 is one position and it is the centre.
#
#   r=0  legs=0  sizes [1]                    ->  1
#   r=1  legs=4  sizes [1,1,2,2,3]            ->  9
#   r=2  legs=8  sizes [1,1,2,2,3,3,4,4,5]    -> 25
#
# Reading it as `range(4 * radius)` — the obvious half-open form — gives 7 and
# 21 and is wrong by exactly the last leg.
def expected(radius):
    return sum(k // 2 + 1 for k in range(4 * radius + 1))

seen_sets = []
for w in walks:
    cx, cy, cz = w["centre"]
    ps = [tuple(p) for p in w["positions"]]
    label = f"r={w['radius']} {w['first']}/{w['second']} at {cx},{cy},{cz}"

    if len(ps) != expected(w["radius"]):
        fail.append(f"{label}: {len(ps)} positions, the leg rule says {expected(w['radius'])}")

    # The first position must be the centre. This is the property a
    # transcription gets wrong, so it is checked rather than assumed.
    if ps[0] != (cx, cy, cz):
        fail.append(f"{label}: first position {ps[0]} is not the centre")

    # y never moves: it is a plane walk.
    if any(p[1] != cy for p in ps):
        fail.append(f"{label}: y moved")

    # Every step is one block, and no position repeats.
    for a, b in zip(ps, ps[1:]):
        if abs(a[0] - b[0]) + abs(a[2] - b[2]) != 1:
            fail.append(f"{label}: {a} -> {b} is not one step")
            break
    if len(set(ps)) != len(ps):
        fail.append(f"{label}: a position repeats")
    seen_sets.append((label, ps))

    print(f"  {label:<38} {len(ps):>4} positions, "
          f"first {ps[0]}, second {ps[1] if len(ps) > 1 else None}, last {ps[-1]}")

# The two r=2 walks share a centre and a radius and differ only in the order of
# the directions, so they must visit the SAME SET in a DIFFERENT ORDER. If they
# came out identical the direction arguments are doing nothing and the whole
# capture would be one walk recorded four times.
swapped = [w for w in walks if w["radius"] == 2]
if len(swapped) == 2:
    a = [tuple(p) for p in swapped[0]["positions"]]
    b = [tuple(p) for p in swapped[1]["positions"]]
    if a == b:
        fail.append("the two r=2 walks are identical — the direction arguments do nothing")
    if set(a) != set(b):
        fail.append("the two r=2 walks cover different sets, which the spiral should not")

if fail:
    for x in fail:
        print(f"  !! {x}")
    sys.exit(1)
print("  SPIRAL-GOLDEN OK")
PYEOF
