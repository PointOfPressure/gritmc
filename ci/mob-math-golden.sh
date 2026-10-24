#!/usr/bin/env bash
# mob-math-golden.sh — Mth.atan2 and its tables, wrapDegrees, rotateIfNecessary, approachDegrees, positiveCeilDiv.
#
# Compiles tools/goldengen/vanilla/MobMathGolden.java against the 26.2 jar and runs it
# twice; the two outputs must agree byte for byte (a golden that iterates a
# hash container would not), then the JSON lands in
# crates/rustmc-entity/golden/mob_math_vectors.json for tests/golden_mob_math.rs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-entity/golden/mob_math_vectors.json}"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR" >&2; exit 1; }
CP="$JAR:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/MobMathGolden.java" 2>&1 | grep -v '^Note' || true
run_once() {
    java -cp "$CP:$WORK" rustmc.golden.MobMathGolden 2>/dev/null | sed 's/.*\[STDOUT\]: //' > "$1"
    python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$1"
}
run_once "$WORK/a.json"
run_once "$WORK/b.json"
cmp -s "$WORK/a.json" "$WORK/b.json" || {
    echo "two runs of mob-math-golden disagree — something in it iterates a map" >&2
    exit 1
}
mkdir -p "$(dirname "$OUT")"
mv "$WORK/a.json" "$OUT"
echo "mob-math vectors -> $OUT"
