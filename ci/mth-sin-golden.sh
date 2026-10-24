#!/usr/bin/env bash
# mth-sin-golden.sh — prove the Mth.SIN lookup table against the JVM.
#
# Two layers in one file: raw PerlinSimplexNoise values for the three stacks
# `Biome` holds, and `coldEnoughToSnow` for every biome over a position grid.
# The harness emits JSON directly (it has to interleave a biome registry walk
# with bitwise float dumps, which is more than a sed pipeline should carry), so
# this script only compiles, runs and strips the server's log prefix.
#
# Like biome-zoom-golden.sh this needs no in-package access — every class it
# touches is public — so it runs against the signed jar directly.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-worldgen/golden/mth_sin_vectors.json}"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR" >&2; exit 1; }
CP="$JAR:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/MthSinGolden.java" 2>&1 | grep -v '^Note' || true

# Regenerate twice and refuse to write unless the two agree (ORACLE.md 5b
# rule 10). Determinism is not a property to assume of a harness that walks
# registries: `HolderLookup.listElements()` has no stable order between JVM
# runs, and an oracle that churns hides a real regression inside its own
# noise. This check is what found fault #5 — a feature ordering sorted from
# the wrong biome list — after reading the harness had confidently missed it.
run_once() {
    java -cp "$CP:$WORK" MthSinGolden 2>/dev/null | sed 's/.*\[STDOUT\]: //' > "$1"
    python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$1"
}

run_once "$WORK/a.json"
run_once "$WORK/b.json"
cmp -s "$WORK/a.json" "$WORK/b.json" || {
    echo "two runs of mth-sin-golden disagree — something in it iterates a map" >&2
    exit 1
}
mv "$WORK/a.json" "$OUT"
echo "mth sin vectors -> $OUT"
