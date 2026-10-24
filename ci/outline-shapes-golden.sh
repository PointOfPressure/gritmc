#!/usr/bin/env bash
# outline-shapes-golden.sh — dump per-state OUTLINE geometry (`getShape`) from the jar.
#
#     ./ci/outline-shapes-golden.sh [out.json]
#
# Produces `data/vanilla/outline_shapes.json`, the shapes
# `ClipContext.Block.OUTLINE` ray-traces against — `BoatItem.use`'s
# `getPlayerPOVHitResult` is its first reader. Same carrier as
# `collision_shapes.json` (see `ci/collision-shapes-golden.sh` for why the
# grid, not the box list, is the shape), so `crate::collide::CollisionShapes`
# loads both. `-1` rows are states whose outline is not a function of the
# state alone — an offset function, or a `getShape` that read its context or
# its level — measured by `tools/goldengen/vanilla/OutlineShapesGolden.java`
# with recording proxies, never listed by hand.
#
# A runtime extract under `data/vanilla/`, gitignored, regenerated per tree by
# `ci/gen-gameplay-tables.sh` and stamped by `ci/provenance.sh`.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/outline_shapes.json}"

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
    "$ROOT/tools/goldengen/vanilla/CollisionShapesGolden.java" \
    "$ROOT/tools/goldengen/vanilla/OutlineShapesGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx4G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.OutlineShapesGolden 2>/dev/null > "$1"
    test -s "$1"
}
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of OutlineShapesGolden disagree — it is reading something ambient" >&2
    exit 1
}
mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
print(f"outline shapes -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['states']} states -> {d['distinctShapes']} distinct shapes, {d['refusedStates']} refused")
print(f"  {len(d['offsetStates'])} offset states carried unshifted, {len(d['offsetGolden'])} golden offsets")
print(f"  {len(d['offsetBlocks'])} offset blocks, {len(d['contextBlocks'])} context readers, "
      f"{len(d['levelBlocks'])} level readers")
fail = []
if len(d["rows"]) != d["states"]:
    fail.append(f"{len(d['rows'])} rows for {d['states']} states")
# Each detector must have fired at least once, or it has stopped detecting:
# 26.2 has offset flowers, a context-reading scaffolding and a level-reading
# shulker box (its block entity), and a zero here would read exactly like "none of them".
for k, must in (("offsetBlocks", "minecraft:poppy"), ("contextBlocks", "minecraft:scaffolding"),
                ("levelBlocks", "minecraft:shulker_box")):
    if must not in d[k]:
        fail.append(f"{must} is not in {k} — that detector has gone blind")
# The offset arm: most offset blocks must be carried (poppy is), or the
# consistency probe has gone wrong and every flower is refused again.
if not any(True for r in d["offsetStates"]):
    fail.append("no offset state is carried — the base-shape consistency probe refuses them all")
if d["distinctShapes"] < 50:
    fail.append(f"only {d['distinctShapes']} distinct shapes — the dedup key is too coarse")
if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PY
