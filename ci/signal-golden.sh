#!/usr/bin/env bash
# signal-golden.sh — regenerate `getSignal`/`getDirectSignal` from the jar.
#
# Produces `data/vanilla/signal.json`: for every block state, how much redstone
# signal it emits in each of the six directions, weak and strong. See
# `tools/goldengen/vanilla/SignalGolden.java` for what the table can and cannot
# answer — the short version is that it carries every state whose answer is a
# function of its own state, and **refuses** the states whose answer is a
# function of the world or of a block entity, by name, so a hand port knows
# exactly what it owes.
#
# Cheap, unlike `ci/shape-golden.sh`: one pass, 32,366 states times twelve
# calls, no cross product and no parallelism. Seconds. It still runs twice and
# demands the two agree, because a harness that iterates a hash map in
# encounter order is a defect this project has shipped before and the check
# costs nothing.
#
# Same signature-stripping and same unanchored-grep warning as
# `ci/shape-golden.sh`: the dumper lives in vanilla's own package and the JVM
# refuses to mix an unsigned class into a signed one, and `Bootstrap.bootStrap()`
# re-emits stdout through log4j with a prefix.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/signal.json}"

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

mkdir -p "$(dirname "$OUT")"
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/SurfaceChunkGolden.java" \
    "$ROOT/tools/goldengen/vanilla/SignalGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx4G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.SignalGolden 2>/dev/null > "$1"
    test -s "$1"
}

run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of SignalGolden disagree — something in it depends on iteration order" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
carried, refused, quiet = len(d["table"]), d["refusedStates"], d["quiet"]
print(f"signal -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['states']} states: {carried} carried, {refused} refused, {quiet} quiet")
total = carried + refused + quiet
if total != d["states"]:
    print(f"  !! carried + refused + quiet = {total}, not {d['states']}")
    sys.exit(1)
print(f"  {d['sources']} carried states are isSignalSource")
for r in d["refused"]:
    print(f"    refused {r['states']:>6}  {r['reads']}  e.g. {', '.join(r['blocks'][:4])}")
PY
