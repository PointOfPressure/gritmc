#!/usr/bin/env bash
# block-entity-golden.sh — regenerate the block -> block-entity-type table.
#
# `BlockEntityType.validBlocks` is the only place vanilla records which blocks
# make which block entity, and it is in neither `blocks.json` nor
# `registries.json`. See `tools/goldengen/vanilla/BlockEntityGolden.java`'s
# header for why this is extracted rather than transcribed: four of the
# forty-nine registrations expand a `ColorCollection` or a
# `WeatheringCopperCollection` that the bytecode never enumerates, and a hand
# expansion that drops one block is a container that keeps nothing with no gate
# able to see it.
#
# Same signature-stripping and same two-run agreement check as
# `ci/block-props-golden.sh`, for the same reasons — read that file's header.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/block_entities.json}"

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
    "$ROOT/tools/goldengen/vanilla/BlockEntityGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.BlockEntityGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. `BuiltInRegistries.BLOCK_ENTITY_TYPE` is a
# `MappedRegistry` and iterates in registration order, but the reflection over
# declared fields in `itemSlots` is not ordered by anything the JVM promises,
# so a class that grew a second `NonNullList` would make this file churn. That
# churn is what this catches; a real regression would otherwise hide inside it.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of BlockEntityGolden disagree — something in it iterates a map" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
TYPES="$(grep -oP '"types": \K[0-9]+' "$OUT")"
BLOCKS="$(grep -oP '"blocks": \K[0-9]+' "$OUT" | head -1)"
UPD="$(grep -oP '"withUpdatePacket": \K[0-9]+' "$OUT")"
echo "block entities -> $OUT ($TYPES types over $BLOCKS blocks, $UPD with a non-null getUpdatePacket; identical across two runs)"
