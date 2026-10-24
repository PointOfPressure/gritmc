#!/usr/bin/env bash
# surface-golden.sh — regenerate the surface golden vectors from the jar.
#
# Separate from `goldengen` because the dumper has to live inside vanilla's
# own package to reach `SurfaceSystem`'s protected members, and the JVM
# refuses to mix an unsigned class into a signed package. So this compiles
# against a signature-stripped copy of the jar, exactly as
# `ci/bench-vanilla.sh` does. Same bytecode; the signature only certifies
# provenance.
#
# Reflection would avoid the stripping, but it would also mean naming those
# methods as strings — and a typo there is a silent zero rather than a
# compile error, which is the failure mode this whole project is built to
# avoid.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-worldgen/golden/surface_chunk_vectors.json}"

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

javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/SurfaceChunkGolden.java" 2>&1 | grep -v '^Note' || true

# `--find <seed> <radius> <block> [biome]` and `--survey <seed> <radius>`
# search instead of dumping. Used to choose the chunk list itself — see the
# CHUNKS comment in the dumper.
#
# **Grep these unanchored.** `Bootstrap.bootStrap()` redirects System.out and
# System.err into log4j, which writes them back to stdout with a
# `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix. An anchored `grep -E "^NEW"`
# matches nothing and exits 1, which looks exactly like "the search found
# nothing" — that cost a 77-minute survey run once.
#
# `--chunk <seed> <cx> <cz> <outfile>` dumps one named chunk at both
# intermediate stages, for `ci/fill-compare.sh`'s stage ladder. It writes the
# file itself rather than to stdout — see the comment on that branch.
if [ "${1:-}" = "--find" ] || [ "${1:-}" = "--probe" ] || [ "${1:-}" = "--survey" ] \
   || [ "${1:-}" = "--chunk" ]; then
    exec java -Drustmc.data="$VDIR/generated-data" ${RUSTMC_JAVA_PROPS:-} -cp "$CP:$WORK" net.minecraft.world.level.levelgen.SurfaceChunkGolden "$@"
fi


# Regenerate twice and refuse to write unless the two agree (ORACLE.md 5b
# rule 10). Determinism is not a property to assume of a harness that walks
# registries: `HolderLookup.listElements()` has no stable order between JVM
# runs, and an oracle that churns hides a real regression inside its own
# noise. This check is what found fault #5 — a feature ordering sorted from
# the wrong biome list — after reading the harness had confidently missed it.
run_once() {
    java -Drustmc.data="$VDIR/generated-data" ${RUSTMC_JAVA_PROPS:-} -cp "$CP:$WORK" net.minecraft.world.level.levelgen.SurfaceChunkGolden 2>/dev/null > "$1"
    test -s "$1"
}

run_once "$WORK/a.json"
run_once "$WORK/b.json"
cmp -s "$WORK/a.json" "$WORK/b.json" || {
    echo "two runs of surface-chunk-golden disagree — something in it iterates a map" >&2
    exit 1
}
mv "$WORK/a.json" "$OUT"
echo "surface chunk vectors -> $OUT"
