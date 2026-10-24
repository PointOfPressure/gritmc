#!/usr/bin/env bash
# biome-zoom-golden.sh — regenerate the BiomeManager fuzz vectors.
#
# Unlike the other surface dumpers this one needs no in-package access, so it
# runs against the signed jar directly.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-worldgen/golden/biome_zoom_vectors.json}"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR" >&2; exit 1; }
CP="$JAR:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/BiomeZoomGolden.java" 2>&1 | grep -v '^Note' || true
java -cp "$CP:$WORK" BiomeZoomGolden 2>/dev/null | sed 's/.*\[STDOUT\]: //' | grep '^SEED' > "$WORK/raw.txt"
python3 - "$WORK/raw.txt" "$OUT" <<'PY'
import json, sys
out = {}
for line in open(sys.argv[1]):
    parts = line.split()
    cells = {}
    for tok in parts[2:]:
        pos, q = tok.split('=')
        x, z = pos.split(',')
        qx, qy, qz = q.split('/')
        cells[f'{x},{z}'] = [int(qx), int(qy), int(qz)]
    out[parts[1]] = cells
json.dump(out, open(sys.argv[2], 'w'), indent=0)
PY
echo "biome zoom vectors -> $OUT"
