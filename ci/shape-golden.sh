#!/usr/bin/env bash
# shape-golden.sh — regenerate `updateShape` from the jar.
#
# Produces `data/vanilla/update_shape.json`: for every block state and every
# direction a change can come from, what vanilla turns that state into. See
# `tools/goldengen/vanilla/ShapeGolden.java` for what the table can and cannot
# answer — the short version is that it carries the *destruction* family, the
# pairs whose only alternative answer is one other state, and refuses anything
# whose rule reads more of the world than its own neighbour.
#
# This is the expensive one. Two passes over 32,366 states — discovery against
# 1,196 block defaults, then an exhaustive sweep for whatever discovery found
# — and it runs twice so the two can be compared. Minutes, not seconds, and
# parallel across every core the machine has. **Threadripper work**, like
# every other extract.
#
# Same signature-stripping and same unanchored-grep warning as
# `ci/survival-golden.sh`: the dumper lives in vanilla's own package and the
# JVM refuses to mix an unsigned class into a signed one, and
# `Bootstrap.bootStrap()` re-emits stdout through log4j with a prefix.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/update_shape.json}"

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
    "$ROOT/tools/goldengen/vanilla/SurfaceChunkGolden.java" \
    "$ROOT/tools/goldengen/vanilla/ShapeGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx4G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.ShapeGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree.
#
# The sweep is parallel across subjects, so this is not a formality: an
# accumulator shared between threads, or a map iterated in encounter order,
# would show up here and nowhere else. `SurvivalGolden` found exactly that
# class of fault the first two times it ran.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of ShapeGolden disagree — something in it depends on thread order" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
print(f"update_shape -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['states']} states, {d['sensitive']} sensitive (state, direction) pairs")
print(f"  {len(d['table'])} carried, {d['connective']} connective, {d['refusedPairs']} refused")
print(f"  {len(d['classes'])} distinct neighbour sets")
for r in d["refused"][:6]:
    print(f"    refused {r['pairs']:>6}  {r['reads']}  e.g. {', '.join(r['blocks'][:3])}")
PY
