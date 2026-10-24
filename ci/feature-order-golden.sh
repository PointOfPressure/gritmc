#!/usr/bin/env bash
# feature-order-golden.sh — dump the global feature order from vanillas own sorter.
#
# The harness emits JSON directly, so this script only compiles, runs and
# strips the server's log prefix.
#
# It compiles alongside `SurfaceChunkGolden` — the biome list the sorter is
# fed comes from the overworld biome *source*, and building that source is
# threaded from one place (ORACLE.md 5b rule 1). That pulls in a class
# declared inside vanilla's own package, so this needs the signature-stripped
# jar like the surface and ore scripts, rather than the signed one it used
# while it was registry-only.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# `DIM=nether` dumps the nether biome source's ordering instead of the
# overworld's, into `feature_order_vectors_nether.json` unless told otherwise.
# The two are different orderings over different biome sets — the nether's five
# biomes are not among the overworld's 55 — so they cannot share a file and a
# feature numbered against the wrong one gets a seed from a list it is not in.
DIM="${DIM:-overworld}"
case "$DIM" in
    overworld) DEFAULT_OUT="$ROOT/crates/rustmc-worldgen/golden/feature_order_vectors.json" ;;
    nether)    DEFAULT_OUT="$ROOT/crates/rustmc-worldgen/golden/feature_order_vectors_nether.json" ;;
    *) echo "unknown dimension '$DIM' (want overworld|nether)"; exit 2 ;;
esac
OUT="${1:-$DEFAULT_OUT}"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR" >&2; exit 1; }

UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi
CP="$UNSIGNED:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/SurfaceChunkGolden.java" \
    "$ROOT/tools/goldengen/vanilla/FeatureOrderGolden.java" 2>&1 | grep -v '^Note' || true

# Regenerate twice and refuse to write unless the two agree (ORACLE.md 5b
# rule 10). Determinism is not a property to assume of a harness that walks
# registries: `HolderLookup.listElements()` has no stable order between JVM
# runs, and an oracle that churns hides a real regression inside its own
# noise. This check is what found fault #5 — a feature ordering sorted from
# the wrong biome list — after reading the harness had confidently missed it.
run_once() {
    java -cp "$CP:$WORK" FeatureOrderGolden "$DIM" 2>/dev/null | sed 's/.*\[STDOUT\]: //' > "$1"
    python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$1"
}

run_once "$WORK/a.json"
run_once "$WORK/b.json"
cmp -s "$WORK/a.json" "$WORK/b.json" || {
    echo "two runs of feature-order-golden disagree — something in it iterates a map" >&2
    exit 1
}
mv "$WORK/a.json" "$OUT"
echo "feature order vectors ($DIM) -> $OUT"
