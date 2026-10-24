#!/usr/bin/env bash
# chunk-send-ties-golden.sh — the order PlayerChunkSender breaks distance ties
# in, taken from the jar's own collectChunksToSend.
#
# `collectChunksToSend` compares on `ChunkPos.distanceSquared` alone, so every
# chunk at an equal distance goes out in the order the pending
# `LongOpenHashSet` streams it (unthrottled: a stable `sorted`; throttled:
# Guava's `Comparators.least`, a quickselect). That order is neither (x, z)
# nor (z, x), and no reading of the comparator can recover it.
#
# So this does not describe it, it records it: `ChunkSendTiesGolden` calls the
# real private method, through the real set and the real selector, over
# scripted add/drop/collect sequences — whole-disc joins drained batch by
# batch, and random churn that grows, shrinks and backward-shifts the table —
# and writes every collect's result. `chunksender.rs` replays the same script
# through `ChunkSender` and must reproduce every line.
#
# Run twice and compared: the driver seeds its own `Random`, so two runs that
# differ mean something in the path is not deterministic and the golden is
# not a golden.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/chunk_send_ties.txt}"
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
javac -nowarn -XDignore.symbol.file -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/ChunkSendTiesGolden.java" 2>&1 | grep -v '^Note' || true
run() {
    java -Xmx2G -cp "$CP:$WORK" net.minecraft.server.network.ChunkSendTiesGolden 2>/dev/null > "$1"
    test -s "$1"
}
run "$WORK/out.txt"
run "$WORK/again.txt"
cmp -s "$WORK/out.txt" "$WORK/again.txt" || {
    echo "two runs of ChunkSendTiesGolden disagree — the path is not deterministic" >&2
    exit 1
}
# The positive control on the golden itself: at least one collect must return
# two equal-distance chunks in an order that is neither (x, z) nor (z, x)
# ascending, or the golden cannot tell the port from either stand-in.
python3 - "$WORK/out.txt" <<'PYEOF'
import sys
cx = cz = None
xz = zx = collects = 0
for line in open(sys.argv[1]):
    if not line.startswith("C "):
        continue
    head, _, tail = line.partition(":")
    _, cx, cz, _, _ = head.split()
    cx, cz = int(cx), int(cz)
    v = list(map(int, tail.split()))
    pos = list(zip(v[0::2], v[1::2]))
    collects += 1
    d = lambda p: (p[0] - cx) ** 2 + (p[1] - cz) ** 2
    for a, b in zip(pos, pos[1:]):
        if d(a) == d(b):
            if a > b:
                xz += 1
            if (a[1], a[0]) > (b[1], b[0]):
                zx += 1
print(f"{collects} collects; adjacent equal-distance pairs out of (x,z) order: {xz}, out of (z,x) order: {zx}")
if collects == 0 or xz == 0 or zx == 0:
    sys.exit("the golden cannot distinguish the port from a coordinate tie-break")
PYEOF
mv "$WORK/out.txt" "$OUT"
echo "chunk send ties -> $OUT ($(wc -l < "$OUT") lines, identical across two runs)"
