#!/usr/bin/env bash
# sky-golden.sh — vanilla's own sun angle and sky light level, per tick.
#
# See tools/goldengen/vanilla/SkyGolden.java for what this pins and — more
# importantly — what it does not.
#
# Like ci/mth-sin-golden.sh this touches only public classes, so it runs
# against the signed jar directly and needs no in-package access.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/sky_vectors.json}"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR" >&2; exit 1; }
CP="$JAR:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/SkyGolden.java" 2>&1 | grep -v '^Note' || true

# Twice, and refuse unless they agree — ORACLE.md 5b rule 10. This one walks a
# registry lookup, and `HolderLookup.listElements()` has no stable order
# between JVM runs.
run_once() {
    java -cp "$CP:$WORK" SkyGolden 2>/dev/null | sed 's/.*\[STDOUT\]: //' > "$1"
    python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); assert d["samples"], "no samples"' "$1"
}

run_once "$WORK/a.json"
run_once "$WORK/b.json"
cmp -s "$WORK/a.json" "$WORK/b.json" || {
    echo "two runs of sky-golden disagree — something in it iterates a map" >&2
    exit 1
}
mkdir -p "$(dirname "$OUT")"
mv "$WORK/a.json" "$OUT"
python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
s = d["samples"]
print(f"sky vectors: {len(s)} ticks over {d['period_ticks']} -> {sys.argv[1]}")
PY
