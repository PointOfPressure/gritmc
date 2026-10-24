#!/usr/bin/env bash
# entity-move-golden.sh - record vanilla running `Entity.collide`.
#
#     ./ci/entity-move-golden.sh [out.json]
#
# Produces `crates/rustmc-server/golden/entity_move.json`: the collider list,
# the three-axis sweep and the full auto-stepping result, for every
# (block layout, movement probe) pair, taken by calling the real methods on a
# real `net.minecraft.world.entity.Entity` in a real `Level`. So
# `crates/rustmc-server/src/collide.rs` is compared against vanilla's bytecode
# rather than against a second reading of it.
#
# **The output is a golden vector and is committed**, unlike the tables
# `ci/gen-gameplay-tables.sh` regenerates per tree. The distinction is the
# difference between a fixture and an extract: an extract is the server's own
# input, so a stale one silently changes what the server does, whereas this
# file is only ever *compared against* - a stale one makes a test red and
# nothing else. Regenerate it when the pinned version moves, and expect the
# diff to be the whole file when it does.
#
# Same signature-stripping as `ci/block-props-golden.sh`, for the same reason:
# the dumper lives inside vanilla's own package and the JVM refuses to mix an
# unsigned class into a signed one.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/entity_move.json}"

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
    "$ROOT/tools/goldengen/vanilla/EntityMoveGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx4G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.EntityMoveGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. The equivalence sweep draws part of its probe
# set from a `java.util.Random` with a constant seed; if that ever became
# ambient, the `mismatches` count would be sampling rather than measuring and
# a run that happened to miss a bad shape would read exactly like a clean one.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of EntityMoveGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
t = d["table"]
print(f"entity move -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['layouts']} layouts x {d['probes']} probes = {d['cases']} cases")
print(f"  {d['casesWithColliders']} cases saw at least one collider")
print(f"  {d['casesClamped']} cases were clamped, "
      f"{d['casesWhereStepUpMoved']} had auto-step change the answer")
print(f"  entity collisions {d['entityCollisions']}, border shapes {d['borderShapes']}")

fail = []
if len(t) != d["cases"] or not t:
    fail.append(f"{len(t)} rows for {d['cases']} cases")
# A fixture in which nothing ever collides is a fixture about empty space, and
# would be green against an implementation that returns its input unchanged.
if d["casesClamped"] < len(t) // 10:
    fail.append(f"only {d['casesClamped']} of {len(t)} cases were clamped - "
                "most probes are not reaching a block")
if d["casesWithColliders"] < len(t) // 4:
    fail.append(f"only {d['casesWithColliders']} of {len(t)} cases saw a collider")
# Auto-step is the part of `Entity.collide` a sweep-only port would miss
# entirely. If no case exercises it, the fixture cannot tell the two apart.
if d["casesWhereStepUpMoved"] == 0:
    fail.append("no case has auto-step change the answer - the step-up arm is untested")
# Both are assumptions this harness makes and records rather than assumes.
if d["entityCollisions"] != 0:
    fail.append(f"{d['entityCollisions']} entity collisions - the harness world is "
                "supposed to be empty of entities")
if d["borderShapes"] != 0:
    fail.append(f"{d['borderShapes']} probes near the world border - they have "
                "drifted out of the region this fixture is about")
# Every diagonal probe exists to separate the two `axisStepOrder` branches. If
# the layouts never stop one, the order is untested and a fixed-order port
# passes.
diag = [r for r in t if r["probe"].startswith("diag_") and r["moved"] != r["delta"]]
if len(diag) < 20:
    fail.append(f"only {len(diag)} diagonal probes were clamped - the axis order "
                "is not being exercised")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
