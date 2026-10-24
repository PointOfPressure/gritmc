#!/usr/bin/env bash
# collision-shapes-golden.sh — dump per-state collision geometry from the jar.
#
#     ./ci/collision-shapes-golden.sh [out.json]
#
# Produces `data/vanilla/collision_shapes.json`, the table `Entity.move` needs
# and the one thing `block_props.json` cannot carry: actual boxes, not six
# per-face booleans. See `tools/goldengen/vanilla/CollisionShapesGolden.java`
# for what is in it and, more to the point, for the sweep that establishes a
# box list is a lossless carrier for `VoxelShape.collide` in the first place.
#
# **This is a runtime extract, not a fixture.** It goes under `data/vanilla/`,
# it is gitignored, it is regenerated per tree by `ci/gen-gameplay-tables.sh`
# and stamped by `ci/provenance.sh`. The distinction from
# `crates/rustmc-server/golden/entity_move.json` is load-bearing: this file is
# an *input* the server reads, so a stale one silently changes where entities
# stop, whereas the golden is only ever compared against and a stale one can
# do nothing but turn a test red.
#
# Same signature-stripping as `ci/block-props-golden.sh` and the same
# unanchored-grep warning: the dumper lives in vanilla's own package, the JVM
# refuses to mix an unsigned class into a signed one, and `Bootstrap.bootStrap()`
# re-emits stdout through log4j with a prefix.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/collision_shapes.json}"

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
    "$ROOT/tools/goldengen/vanilla/CollisionShapesGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx4G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.CollisionShapesGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. The equivalence sweep draws part of its probe
# set from a `java.util.Random` with a constant seed; if that ever became
# ambient, the `mismatches` count would be sampling rather than measuring and
# a run that happened to miss a bad shape would read exactly like a clean one.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of CollisionShapesGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PY'
import json, sys

d = json.load(open(sys.argv[1]))
box = d["boxEquivalence"]
grid = d["gridEquivalence"]
over = d["overlapEquivalence"]
print(f"collision shapes -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['states']} states -> {d['distinctShapes']} distinct shapes"
      f" ({d['emptyStates']} with no collision, {d['dynamicStates']} dynamic)")
print(f"  grid  equivalence: {grid['probes']:,} probes, {grid['mismatches']} mismatches")
print(f"  boxes equivalence: {box['probes']:,} probes, {box['mismatches']} mismatches"
      f" ({box['overlapping']} with the query box already inside the shape)")
print(f"  admission  test: {over['probes']:,} probes, {over['mismatches']} mismatches")
print(f"  grid lines differ from box corners on {d['coordsDifferFromBoxCorners']} shapes")
print(f"  {len(d['dynamicBlocks'])} dynamic-shape blocks: "
      f"{', '.join(d['dynamicBlocks'][:6])}")
print(f"  {len(d['contextSensitiveBlocks'])} classes override getCollisionShape")

fail = []
# The grid sweep is the table's whole warrant. A single mismatch means
# `coords` + `voxels` is not a lossless carrier for this version and the port
# must not ship.
if grid["mismatches"] != 0:
    fail.append(f"{grid['mismatches']} grid mismatches: {grid['examples'][:2]}")
# A sweep that ran no probes would report zero mismatches and mean nothing.
if grid["probes"] < 1_000_000:
    fail.append(f"only {grid['probes']} probes — the sweep is not exercising the shapes")
# `BlockCollisions` decides membership with `joinIsNotEmpty(..., AND)`, whose
# coordinate merging carries its own 1.0E-7 tolerance. A mismatch means plain
# strict overlap is the wrong predicate and blocks are entering or leaving the
# collider list that vanilla would not move.
if over["mismatches"] != 0:
    fail.append(f"{over['mismatches']} admission-test mismatches: {over['examples'][:2]}")
if over["probes"] < 100_000:
    fail.append(f"only {over['probes']} admission probes")
# **The box arm must stay red, and this is not a joke assertion.** It is the
# only thing standing between a later reader and the intuitive-but-wrong
# conclusion that `toAabbs()` would have done. If a version ever makes the box
# list sufficient, that is a finding to be recorded deliberately, not a green
# to be absorbed silently — and until then a zero here means the sweep has
# stopped probing the overlapping case and the grid arm's zero means nothing
# either, because both arms share the same probes.
if box["mismatches"] == 0:
    fail.append("the box-list arm agrees everywhere — the sweep has stopped "
                "probing boxes already inside a shape, so the grid arm's zero "
                "is measuring nothing")
# Every box-list disagreement was an already-overlapping box on 26.2. A
# disagreement that is NOT that is a second mechanism nobody has looked at.
if box["mismatches"] != box["overlapping"]:
    outside = box["mismatches"] - box["overlapping"]
    fail.append(f"{outside} box-list disagreements where the query box is NOT "
                f"inside the shape — a second mechanism: {box['examples'][:2]}")
# The census is derived by reflection; an empty one means the signature match
# has gone stale against a renamed method rather than that nothing overrides.
if len(d["contextSensitiveBlocks"]) == 0:
    fail.append("the getCollisionShape census is empty — the reflection filter is stale")
if d["distinctShapes"] < 50:
    fail.append(f"only {d['distinctShapes']} distinct shapes — the dedup key is too coarse")
# One line in `collect_block_collisions` reads this and nothing else does, so
# an empty list turns that line into dead code that always takes the skip.
if len(d["movingPistonStates"]) == 0:
    fail.append("no moving_piston states — the edge-cell rule would never fire")
if len(d["rows"]) != d["states"]:
    fail.append(f"{len(d['rows'])} rows for {d['states']} states")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PY
