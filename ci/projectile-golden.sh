#!/usr/bin/env bash
# projectile-golden.sh — record vanilla running the projectile path's arithmetic.
#
#     ./ci/projectile-golden.sh [out.json]
#
# Produces `crates/rustmc-server/golden/projectile.json`: the launch
# directions, the inaccuracy draw streams, `lerpRotation`, the bow's power
# curve, the crossbow's shot pitch, `AABB.clip`, `VoxelShape.clip` and
# `BlockGetter.traverseBlocks`, taken by calling the real methods in the
# pinned jar. So `crates/rustmc-server/src/projectile/` is compared against
# vanilla's bytecode rather than against a second reading of it.
#
# **What this does and does not settle.** Everything here is reachable with no
# `Level`, which is why the harness is 300 lines and not 900. The whole-tick
# assembly — `AbstractArrow.tick`, `ProjectileUtil.computeMargin` and
# `getManyEntityHitResult` — needs the `MoveLevel` treatment
# `ci/entity-move-golden.sh` gives `Entity.collide`, and until that exists
# `crates/rustmc-server/tests/projectile_flight.rs` grades the assembly
# against this lane's own reading and says so in its header.
#
# Signature stripping is not needed and neither is the unsigned jar, because
# the harness class is in package `rustmcgolden` rather than in one of
# vanilla's. A class of ours inside `net.minecraft.world.level.levelgen`
# makes `Bootstrap.bootStrap()` throw `SecurityException: signer information
# does not match`, and this harness needs `Bootstrap` for `CrossbowItem`'s
# static initialiser. Measured, not assumed: that is the exception this
# script's first run produced.
#
# **The output is a golden vector and is committed.** Regenerate it when the
# pinned version moves and expect the diff to be the whole file when it does.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/projectile.json}"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch" >&2; exit 1; }
CP="$JAR:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

mkdir -p "$(dirname "$OUT")"
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/ProjectileGolden.java" 2>&1 | grep -v '^Note' || true

# Twice, and they must agree. Determinism is not a property to assume of a
# harness that boots `Bootstrap` and reflects into two classes: an oracle that
# churns hides a real regression inside its own noise. ORACLE.md 5b rule 10.
run_once() {
    java -Xmx2G -cp "$CP:$WORK" \
        rustmcgolden.ProjectileGolden 2>/dev/null \
        | sed 's/.*\[STDOUT\]: //' > "$1"
    python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$1"
}

run_once "$WORK/a.json"
run_once "$WORK/b.json"
cmp -s "$WORK/a.json" "$WORK/b.json" || {
    echo "two runs of projectile-golden disagree — something in it iterates a map" >&2
    exit 1
}
mv "$WORK/a.json" "$OUT"

python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
# A floor under every section, because an empty array is a golden that cannot
# fail and this file is compared row for row. The counts are the sweep's own
# shape, so a harness edit that drops a loop reds here rather than passing.
want = {
    "launchDirections": 13 * 25,
    "triangleStreams": 5 * 4,
    "lerpRotation": 15 * 15,
    "bowPower": 41,
    "crossbowShotPitch": 3,
    "aabbClip": 5 * 11,
    "aabbBoxes": 5,
    "aabbSegments": 11,
    "voxelClip": 4 * 2 * 11,
    "voxelShapes": 4,
    "shapePositions": 2,
    "traverse": 9,
}
bad = [(k, len(d.get(k, [])), v) for k, v in want.items() if len(d.get(k, [])) != v]
if bad:
    for k, got, exp in bad:
        print(f"projectile-golden: {k} has {got} rows, expected {exp}", file=sys.stderr)
    sys.exit(1)
hits = sum(1 for r in d["aabbClip"] if r["hit"])
vhits = sum(1 for r in d["voxelClip"] if r["hit"])
inside = sum(1 for r in d["voxelClip"] if r.get("inside"))
cells = sum(len(r["cells"]) for r in d["traverse"])
print(f"projectile-golden: {sum(want.values())} rows; "
      f"aabbClip {hits}/{len(d['aabbClip'])} hit; "
      f"voxelClip {vhits}/{len(d['voxelClip'])} hit, {inside} inside; "
      f"traverse {cells} cells over {len(d['traverse'])} walks")
# Each of these three would be a golden with nothing in it to compare.
assert hits > 0 and vhits > 0 and inside > 0 and cells > len(d["traverse"])
PY
echo "projectile vectors -> $OUT"
