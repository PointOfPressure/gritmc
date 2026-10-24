#!/usr/bin/env bash
# beard-golden.sh — vanilla's `Beardifier`, as a per-component golden.
#
# The beard is the first term in this port with no byte-identical gate: it is
# meant to change terrain, so the composed world diff cannot be its test, and
# a composed residual cannot say which of five pieces is wrong. This dumps
# each piece that has a vanilla-side oracle — the 13,824-entry kernel as raw
# float bits, `getBeardContribution` and `getBuryContribution` over swept
# grids, and `Beardifier.compute` for a piece of each `TerrainAdjustment` and
# for jigsaw junctions.
#
# `forStructuresInChunk` is NOT covered here and cannot be: it needs a
# `StructureManager`. See the header of `BeardGolden.java`.
#
# Runs a JVM, so it goes on the build machine like everything else that runs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-worldgen/golden/beard_vectors.json}"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch" >&2; exit 1; }
CP="$JAR:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/BeardGolden.java" 2>&1 |
    grep -v '^Note' || true

# Regenerate twice and refuse to write unless the two runs agree — ORACLE.md
# rule 10. The beard has no registry walk in it and ought to be trivially
# deterministic, which is exactly why the check is cheap to keep: an oracle
# that churns hides a real regression inside its own noise, and the cost of
# finding that out later is a withdrawn number.
run_once() {
    java -cp "$CP:$WORK" BeardGolden 2>/dev/null | sed 's/.*\[STDOUT\]: //' > "$1"
    python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$1"
}

run_once "$WORK/a.json"
run_once "$WORK/b.json"
cmp -s "$WORK/a.json" "$WORK/b.json" || {
    echo "two runs of beard-golden disagree — something in it iterates a map" >&2
    exit 1
}
mv "$WORK/a.json" "$OUT"
echo "beard vectors -> $OUT"
