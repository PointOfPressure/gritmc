#!/usr/bin/env bash
# hash-order-golden.sh — java.util.HashSet iteration order, from the JVM.
#
# `TreeFeature` collects its logs and foliage into `Sets.newHashSet()` and a
# decorator picks from the result, so the set's iteration order is behaviour.
# `crates/rustmc-worldgen/src/java_hash_set.rs` reproduces it; this dumps what
# the real JVM does, because a reproduction checked only against itself is a
# reproduction of nothing.
#
# This one DOES bootstrap and build `VanillaRegistries.createLookup()`, since
# `isStructureChunk` takes a real `ChunkGeneratorStructureState`. So it is a
# few seconds, and any diagnostic output goes through log4j — see the grep
# note below.
#
# Same signature-stripping as `ci/surface-chunk-golden.sh`, for the same
# reason: the dumper lives inside vanilla's own package and the JVM refuses to
# mix an unsigned class into a signed one.
#
# **Grep any diagnostic output unanchored.** `Bootstrap.bootStrap()` redirects
# System.out through log4j, which re-emits it with a
# `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix, so `grep -E "^SOMETHING"`
# matches nothing and exits 1 — indistinguishable from a clean run that found
# nothing. And pipe through `grep --line-buffered` if anything downstream
# wants to stop early; plain grep block-buffers into a pipe.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/structure_placement.json}"

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
    "$ROOT/tools/goldengen/vanilla/StructurePlacementGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.structure.StructurePlacementGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree.
#
# The first two runs of this script did *not*: `HolderLookup.listElements()`
# has no stable order between JVM runs, and a config's position in that list
# becomes the feature index handed to `setFeatureSeed`. Every case drew from a
# different stream each time the file was rebuilt, so the vectors churned and
# a real regression would have been invisible inside the churn. Sorted by name
# in the dumper now — and checked here, because the next unstable iteration
# order will not announce itself either.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of StructurePlacementGolden disagree — something in it iterates a map" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
echo "structure placement -> $OUT (identical across two runs)"
