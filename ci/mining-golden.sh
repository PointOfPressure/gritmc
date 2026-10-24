#!/usr/bin/env bash
# mining-golden.sh — the per-block facts the dig pipeline reads, from the jar.
#
#     ./ci/mining-golden.sh [OUT]      default data/vanilla/block_mining.json
#
# `tools/goldengen/vanilla/MiningGolden.java` walks `BuiltInRegistries.BLOCK`
# and writes, per block, `getDestroySpeed` (the destroy time),
# `requiresCorrectToolForDrops`, whether it is a `GameMasterBlock`, and which
# class declares `playerWillDestroy` / `playerDestroy` / `spawnAfterBreak`.
# `crate::mining` reads it for `BlockBehaviour.getDestroyProgress` and
# `ServerPlayerGameMode.destroyBlock`.
#
# Run twice and compared, like every generator here: a table that differs
# between two runs of the same jar is iterating something unordered.
#
# The spot checks at the end are the table's red-when-broken evidence. Each
# one is a value read from the block's own `Properties` call in the pinned jar
# (`Blocks.<clinit>`), and each would move if the generator read the wrong
# state, the wrong field, or ran with a registry that was never bound.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/block_mining.json}"
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
    "$ROOT/tools/goldengen/vanilla/MiningGolden.java" 2>&1 | grep -v '^Note' || true
run() {
    java -Xmx2G -cp "$CP:$WORK" net.minecraft.world.level.levelgen.MiningGolden \
        2>"$WORK/err" > "$1"
    test -s "$1" || { sed -n "1,20p" "$WORK/err" >&2; return 1; }
}
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of MiningGolden disagree — the block order is not stable" >&2
    exit 1
}
python3 - "$WORK/out.json" <<'PYEOF'
import json, sys
doc = json.load(open(sys.argv[1]))
rows = {r["block"]: r for r in doc["table"]}
want = {
    "minecraft:grass_block": (0.6, False),
    "minecraft:dirt": (0.5, False),
    "minecraft:stone": (1.5, True),
    "minecraft:obsidian": (50.0, True),
    "minecraft:bedrock": (-1.0, False),
    "minecraft:torch": (0.0, False),
    "minecraft:oak_log": (2.0, False),
}
bad = []
for block, (t, req) in want.items():
    r = rows.get(block)
    if r is None or abs(r["destroy_time"] - t) > 1e-6 or r["requires_correct_tool"] != req:
        bad.append(f"{block}: {r}")
if not rows.get("minecraft:command_block", {}).get("game_master"):
    bad.append("minecraft:command_block is not a GameMasterBlock")
if rows.get("minecraft:ice", {}).get("player_destroy") != "net.minecraft.world.level.block.IceBlock":
    bad.append("minecraft:ice does not override playerDestroy")
if doc["blocks"] != len(doc["table"]) or doc["blocks"] < 1000:
    bad.append(f"blocks={doc['blocks']} rows={len(doc['table'])}")
if bad:
    print("mining-golden: spot checks FAILED:\n  " + "\n  ".join(bad), file=sys.stderr)
    sys.exit(1)
print(f"mining-golden: {doc['blocks']} blocks, {doc['unbreakable']} unbreakable, "
      f"{doc['instant']} instant, {doc['requires_correct_tool']} need the correct tool")
PYEOF
mv "$WORK/out.json" "$OUT"
echo "block mining -> $OUT (identical across two runs)"
