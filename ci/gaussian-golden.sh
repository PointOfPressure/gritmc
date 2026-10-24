#!/usr/bin/env bash
# hash-order-golden.sh — java.util.HashSet iteration order, from the JVM.
#
# `TreeFeature` collects its logs and foliage into `Sets.newHashSet()` and a
# decorator picks from the result, so the set's iteration order is behaviour.
# `crates/rustmc-worldgen/src/java_hash_set.rs` reproduces it; this dumps what
# the real JVM does, because a reproduction checked only against itself is a
# reproduction of nothing.
#
# No `Bootstrap.bootStrap()` here: `BlockPos` is a plain value class and
# nothing in this harness touches a registry, so the dump is a few
# milliseconds rather than a few seconds.
#
# Same signature-stripping as `ci/surface-chunk-golden.sh`, for the same
# reason: the dumper lives inside vanilla's own package and the JVM refuses to
# mix an unsigned class into a signed one.
#
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/gaussian.json}"

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
    "$ROOT/tools/goldengen/vanilla/GaussianGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.GaussianGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree.
#
# ORACLE.md rule 10, and it is not ceremony: the first two runs of the hash
# order dumper genuinely disagreed, because a registry it read iterates in JVM
# order. Nothing here reads a registry, which is a reason to expect agreement
# and not a reason to skip checking for it.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of GaussianGolden disagree — something in it iterates a map" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
echo "gaussian -> $OUT (identical across two runs)"
