#!/usr/bin/env bash
# light-golden.sh — regenerate the section-light golden vectors from the jar.
#
# # What this measures, and why it is not a corpus check
#
# The light section of `ClientboundLevelChunkWithLightPacket` is the payload
# of the most common packet in the game, and until 2026-08-18 nothing compared
# it: we classified **0 of 26** sections. The corpus cannot grade it, for two
# reasons that are structural:
#
#   * `trace::observe` drops a chunk packet whose block-entity list is empty,
#     measured at **132 chunk packets in the window, 1 recorded**. That filter
#     is correct and stays.
#   * `ci/t2-gameplay.sh` runs on superflat, whose own header says every
#     column is identical. Light there is a function of y alone, so a
#     comparison of light *values* on it is vacuous by construction — the
#     `710` defect exactly, a check that ran while the code path did not
#     because a superflat world measured the same thing on both sides.
#
# So the observation point is vanilla's `LevelLightEngine` in process, driven
# through the sequence `ThreadedLevelLightEngine` uses, over worlds chosen so
# that light is *not* uniform. The dumped artefact is the packet's own light
# section, built by `ClientboundLightUpdatePacketData` itself.
#
# Same signature-stripping as `ci/ore-golden.sh`, for the same reason: the
# dumper lives inside vanilla's own package and the JVM refuses to mix an
# unsigned class into a signed one.
#
# **Grep any diagnostic output unanchored** — `Bootstrap.bootStrap()` puts
# System.out through log4j, which prefixes every line.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/light_vectors.json}"

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

# Compiled **together with** SurfaceChunkGolden: `LightGolden` reuses its
# biome registry construction rather than carrying a second copy, which is the
# same one-generation-path rule `ci/ore-golden.sh` follows (ORACLE.md 5b).
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/SurfaceChunkGolden.java" \
    "$ROOT/tools/goldengen/vanilla/GenLevel.java" \
    "$ROOT/tools/goldengen/vanilla/LightGolden.java" 2>&1 | grep -v '^Note' || true

# `LIGHT_RADIUS` widens the grid. It exists for `ci/light-radius-sweep.sh`,
# which asks the ORACLE rule 14 question — widen the context until the answer
# stops changing, and quote the converged one — of light rather than assuming
# a 3x3 is enough.
run() {
    java -Drustmc.data="$VDIR/generated-data" -Dlight.radius="${LIGHT_RADIUS:-1}" \
        -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.LightGolden "$1" >/dev/null 2>&1
    test -s "$1"
}

# Twice, and they must agree.
#
# Not decoration. The light engine drains a queue whose order comes out of
# `LongOpenHashSet` and `LeveledPriorityQueue`, and if any of that leaked into
# the *result* the vectors would churn between rebuilds and a real regression
# would be invisible inside the churn. `ci/ore-golden.sh` found exactly that in
# `HolderLookup.listElements()`. Converged light should be order-independent —
# this is the check that says so rather than the assumption that it is.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of LightGolden disagree — the converged light is order-dependent" >&2
    exit 1
}

mkdir -p "$(dirname "$OUT")"
mv "$WORK/out.json" "$OUT"
echo "light vectors -> $OUT (identical across two runs)"
