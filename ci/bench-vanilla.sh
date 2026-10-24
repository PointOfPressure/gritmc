#!/usr/bin/env bash
# bench-vanilla.sh — time vanilla's noise stage, for comparison against
# `worldgen-dump bench`.
#
# The point of this script is that the comparison is only worth anything when
# both halves run on the same machine, in the same session, doing the same
# work. Quoting a remembered "vanilla does about 10 ms a chunk" against a
# number measured here is two unrelated facts, not a measurement.
#
#   ci/bench-vanilla.sh [seed] [chunks] [warmup]
#
# Then compare with:
#
#   cargo build --release -p worldgen-dump
#   ./target/release/worldgen-dump bench <seed> <chunks>
#
# Both report the *noise stage only*: density, cell interpolation, aquifer,
# ore veins, block fill. No surfaces, carvers, features or structures, and no
# disk IO on either side.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SEED="${1:-20260725}"
CHUNKS="${2:-64}"
WARMUP="${3:-32}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }

# The bench class lives in `net.minecraft.world.level.levelgen` so it can
# reach BeardifierMarker and NoiseChunk's protected members without
# reflection — a reflective call per block would time the reflection rather
# than the generator. The JVM refuses to mix an unsigned class into a signed
# package, so it runs against a signature-stripped copy. Same bytecode, same
# behaviour; the signature only certifies provenance.
UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    zip -q -d "$UNSIGNED" 'META-INF/*.SF' 'META-INF/*.RSA' 'META-INF/*.DSA' 2>/dev/null || true
    # Globs are not always honoured by zip -d, so retry the concrete names.
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi

CP="$UNSIGNED:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

javac -nowarn -cp "$CP" -d "$OUT" "$ROOT/tools/goldengen/vanilla/VanillaBench.java" 2>&1 | grep -v '^Note' || true
java -cp "$CP:$OUT" net.minecraft.world.level.levelgen.VanillaBench "$SEED" "$CHUNKS" "$WARMUP" \
    2>/dev/null
