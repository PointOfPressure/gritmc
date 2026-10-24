#!/usr/bin/env bash
# decorate-golden.sh — regenerate the ore-feature golden vectors from the jar.
#
# Compiles `DecorateGolden.java` **together with** `SurfaceChunkGolden.java`: the
# two share one chunk-generation path on purpose, so that a world the ore gate
# runs against is the same world the surface and carver gates measured. Two
# copies of that pipeline is precisely the oracle fault this project keeps
# finding (ORACLE.md 5b).
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
OUT="${1:-$ROOT/crates/rustmc-worldgen/golden/decorate_vectors.json}"

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

javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/SurfaceChunkGolden.java" \
    "$ROOT/tools/goldengen/vanilla/GenLevel.java" \
    "$ROOT/tools/goldengen/vanilla/DecorateGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    # stderr kept, not discarded. It used to go to /dev/null, and when the
    # JVM threw — as it does at seed 31415926 centre (65,62) — the only
    # symptom was an empty file and a bare exit 1, with the exception gone.
    # `|| true` because `set -e` would otherwise kill the script *at this
    # line* when the JVM exits non-zero, before the diagnostic below ever
    # runs — which is why this failed silently even after stderr was kept.
    # `-Ddecorate.region=1` is the default, not an option.
    #
    # Without it the dumper builds a fresh 3x3 per centre and decorates only
    # the middle chunk, so neighbours exist for reads and never decorate —
    # a world no server produces. Measured at seed 31415926 centre (67,62),
    # 11 of that chunk's 59 features behave differently once the
    # neighbourhood decorates, and 20 of the 169 committed vectors were
    # wrong, `patch_bush` by 4 positions against 18.
    #
    # The committed vectors were regenerated with it. If this script defaulted
    # back to chunk mode, a routine `ci/decorate-golden.sh` would silently
    # rewrite them to the old wrong numbers and the gate would then be green
    # against fiction — which is exactly the failure mode this whole
    # investigation was about. Override deliberately via RUSTMC_JAVA_PROPS if
    # a single-chunk dump is genuinely wanted; a later -D wins.
    java -Drustmc.data="$VDIR/generated-data" -Ddecorate.out="$1" \
        -Ddecorate.region=1 ${RUSTMC_JAVA_PROPS:-} -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.DecorateGolden > "$1.log" 2>&1 || true
    if [ ! -s "$1" ]; then
        echo "DecorateGolden wrote nothing; its stderr:" >&2
        tail -40 "$1.log" >&2
        return 1
    fi
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
    echo "two runs of DecorateGolden disagree — something in it iterates a map" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
echo "decorate vectors -> $OUT (identical across two runs)"
