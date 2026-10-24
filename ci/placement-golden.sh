#!/usr/bin/env bash
# placement-golden.sh — extract `Block.getStateForPlacement` per block.
#
# `block_props.json` says `minecraft:oak_stairs` has a `facing` and so does
# `minecraft:furnace`, and there is nothing in a table of *states* that can
# tell you `StairBlock` sets it to `getHorizontalDirection()` and
# `AbstractFurnaceBlock` to its **opposite**. This asks each block instead,
# over a grid of clicks. See `tools/goldengen/vanilla/PlacementGolden.java`
# for what the grid is and what the world it clicks in looks like.
#
# One file, no shared pipeline: unlike the worldgen dumpers this needs no
# generated chunk, so compiling it beside `SurfaceChunkGolden` would buy
# nothing and cost the coupling.
#
# Same signature-stripping as `ci/block-props-golden.sh`, for the same
# reason: the dumper lives inside vanilla's own package and the JVM refuses to
# mix an unsigned class into a signed jar.
#
# **Grep any diagnostic output unanchored.** `Bootstrap.bootStrap()` redirects
# both streams through log4j, which re-emits each line with a
# `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix, so `grep -E "^SOMETHING"`
# matches nothing and exits 1 — indistinguishable from a clean run that found
# nothing. That prefix is also why this dumper writes its JSON to stdout and
# is captured with `>` rather than through a pipe.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/placement.json}"

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
    "$ROOT/tools/goldengen/vanilla/PlacementGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    # stderr kept, not discarded. The dumper throws on purpose when a block
    # reads something its level does not model — that is the design, and the
    # exception names the method and the block. Sending it to /dev/null would
    # leave an empty file and a bare exit 1.
    java -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.PlacementGolden > "$1" 2> "$1.log" || true
    # A `{` first byte, not merely a non-empty file: when the dumper throws,
    # log4j has already claimed stdout and the "empty" file is full of stack
    # trace. `test -s` passes on it and the JSON parse fails much later.
    if [ "$(head -c 1 "$1" 2>/dev/null)" != "{" ]; then
        echo "PlacementGolden did not produce JSON; the last of what it said:" >&2
        head -20 "$1" >&2
        tail -20 "$1.log" >&2
        return 1
    fi
}

# Twice, and they must agree.
#
# `BuiltInRegistries.BLOCK` iterates in registration order and is stable, but
# `HolderLookup.listElements()` — which `accessOf` walks to rebuild the
# registries the level needs — is not, and that instability once churned the
# decorate vectors under a different dumper. Nothing here should depend on it;
# this is how that is checked rather than assumed.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of PlacementGolden disagree — something in it iterates a map" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
echo "placement table -> $OUT (identical across two runs)"
