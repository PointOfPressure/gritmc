#!/usr/bin/env bash
# explosion-ray-golden.sh — record vanilla running its own explosion ray walk.
#
# One JVM run of `ExplosionRayGolden`, which allocates a `ServerExplosion` and
# a `ServerLevel` subclass without their constructors, sets their fields by
# reflection, and reflectively invokes the private
# `ServerExplosion.calculateExplodedPositions()`. Twelve worlds, and both the
# raw `ObjectArrayList<>(HashSet)` order and the order after
# `Util.shuffle(list, level.random)`.
#
# WHAT THIS BUYS THAT A SECOND READING OF THE BYTECODE DOES NOT.
# Three of the four things the block pass depends on are invisible to a
# transcription check: the `java.util.HashSet` iteration order the position
# list inherits, the fact that the shuffle continues the *same* RandomSource
# stream the 1,352 ray draws came from, and the exact float rounding of
# `(res + 0.3f) * 0.3f` against `0.22500001f`. This records all three from the
# jar itself.
#
# THE ASSERTIONS BELOW ARE THE INSTRUMENT'S OWN FALSIFIABILITY.
# A dumper whose reflection had silently stopped working — a renamed field, a
# `ServerLevel` that no longer answers — would most likely produce *empty*
# position lists for every case, and empty lists compare equal to an empty
# port. So the script refuses:
#
#   * a case set that does not hold every expected name;
#   * `open_air_r4_s1` with no positions, or with the same positions as
#     `open_air_r4_s2` (the seed must reach the geometry);
#   * `floor_r4` equal to `open_air_r4_s1` (the world must reach the geometry);
#   * `obsidian_r4`, `water_r4`, `out_of_bounds_r4` or `zero_radius` with ANY
#     position (each is empty for a different and separately checkable
#     reason, and a dumper that returned the open-air answer everywhere would
#     fail here rather than pass everywhere);
#   * any case whose `shuffled` is not a permutation of its `positions`.
#
# **Grep any diagnostic output unanchored.** `Bootstrap.bootStrap()` redirects
# System.out through log4j.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/explosion_ray.json}"

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
# `sun.reflect.ReflectionFactory` lives in `jdk.unsupported` and is reachable
# without a module flag; javac warns about it and the warning is expected.
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/ExplosionRayGolden.java" 2>&1 \
    | grep -vE '^Note|proprietary|^ *\^|^[0-9]+ warning' || true

run() {
    java -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.ExplosionRayGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. `RandomSource.create(seed)` is deterministic and
# the stub level has no map to iterate, so a disagreement is a bug in the
# dumper — but this project has been surprised by an unstable iteration order
# three times and the check costs one JVM start.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of ExplosionRayGolden disagree — something in it iterates a map" >&2
    exit 1
}

python3 - "$WORK/out.json" <<'PY'
import json, sys

doc = json.load(open(sys.argv[1]))
if doc.get("rays") != 1352:
    sys.exit(f"rays is {doc.get('rays')}, not 16^3 - 14^3 = 1352")

cases = {c["name"]: c for c in doc["cases"]}
expected = {
    "open_air_r4_s1", "open_air_r4_s2", "stone_r4", "obsidian_r4", "floor_r4",
    "water_r4", "air_over_lava_r4", "out_of_bounds_r4", "open_air_r3",
    "floor_r3", "floor_r4_negative", "zero_radius", "wall_east_r4", "ceiling_r4",
}
missing = expected - set(cases)
if missing:
    sys.exit(f"cases missing: {sorted(missing)}")


def triples(flat):
    return list(zip(flat[0::3], flat[1::3], flat[2::3]))


for name, c in cases.items():
    p = triples(c["positions"])
    s = triples(c["shuffled"])
    if len(p) != c["count"]:
        sys.exit(f"{name}: count {c['count']} against {len(p)} positions")
    if sorted(p) != sorted(s):
        sys.exit(f"{name}: shuffled is not a permutation of positions")
    if len(set(p)) != len(p):
        sys.exit(f"{name}: the position list has a duplicate, so it is not a set")
    ns = triples(c["fireNoShuffle"])
    fs = triples(c["fireAfterShuffle"])
    for label, f in (("fireNoShuffle", ns), ("fireAfterShuffle", fs)):
        if not set(f) <= set(p):
            sys.exit(f"{name}: {label} put fire somewhere the blast never reached")

# Each of these is empty for a DIFFERENT reason, and a dumper answering the
# same thing everywhere fails at least one of the two groups.
for name, why in [
    ("obsidian_r4", "(1200 + 0.3) * 0.3 = 360.09 on the first step"),
    ("water_r4", "the fluid arm of Math.max is 100.0, so 30.09 a step"),
    ("out_of_bounds_r4", "isInWorldBounds is false, so every ray breaks"),
    ("zero_radius", "f starts at 0.0 and the while never runs"),
]:
    if cases[name]["count"] != 0:
        sys.exit(f"{name} found {cases[name]['count']} positions; it should be empty — {why}")

for name in ["open_air_r4_s1", "open_air_r4_s2", "floor_r4", "air_over_lava_r4",
             "open_air_r3", "floor_r3", "floor_r4_negative", "wall_east_r4",
             "ceiling_r4"]:
    if cases[name]["count"] == 0:
        sys.exit(f"{name} found nothing — the dumper is answering empty for everything")

if cases["stone_r4"]["count"] != 1:
    sys.exit("stone_r4 is not 1 position; (6.0 + 0.3) * 0.3 = 1.89 a step "
             f"should leave exactly the origin, got {cases['stone_r4']['count']}")

if cases["open_air_r4_s1"]["positions"] == cases["open_air_r4_s2"]["positions"]:
    sys.exit("two seeds gave one blast — the stream does not reach the geometry")
if cases["open_air_r4_s1"]["positions"] == cases["floor_r4"]["positions"]:
    sys.exit("two worlds gave one blast — the world does not reach the geometry")
if cases["open_air_r4_s1"]["positions"] == cases["open_air_r4_s1"]["shuffled"]:
    sys.exit("the shuffle left the list alone")

# THE FIRE PASS INHERITS THE SHUFFLE, and this is where that is recorded from
# vanilla rather than argued. `explode()` holds one List: `Util.shuffle`
# permutes it in place inside `interactWithBlocks` and `createFire` is handed
# that same object. Both arms below start from the state the 1,352 ray draws
# left the stream in, so the only difference between them is the shuffle — its
# n-1 draws and its permutation.
fired = [n for n in cases
         if cases[n]["fireNoShuffle"] or cases[n]["fireAfterShuffle"]]
if not fired:
    sys.exit("no case placed any fire — createFire was never exercised")
moved = [n for n in fired
         if cases[n]["fireNoShuffle"] != cases[n]["fireAfterShuffle"]]
if not moved:
    sys.exit("every case put fire in the same places with and without the "
             "shuffle — the fire pass does not inherit it, or the two arms "
             "are not actually running from the same stream state")

# `wall_east` must be asymmetric about x — obsidian at x >= 2 and nothing
# anywhere else — which is the axis `floor_r4` cannot test. A port that
# transposed two axes in its resistance lookup passes every symmetric world and
# fails here.
w = triples(cases["wall_east_r4"]["positions"])
east = max(x for x, _, _ in w)
west = min(x for x, _, _ in w)
if (east - 0) >= (0 - west):
    sys.exit(f"wall_east_r4 reaches {east} east and {-west} west — the wall is "
             "not being read, or the axes are transposed")
# `ceiling_r4` terminates on `isInWorldBounds`, which BREAKS the ray, where a
# hard block only subtracts from it. Nothing may be collected at or above the
# limit.
cl = triples(cases["ceiling_r4"]["positions"])
if not cl:
    sys.exit("ceiling_r4 collected nothing at all")
if max(y for _, y, _ in cl) >= 68:
    sys.exit("ceiling_r4 collected a position at or above the build height, so "
             "the bounds check is not breaking the ray")

# The floor case must be asymmetric about y, which is what a transposed-axis
# port would not be: stone below y = 64 costs 1.89 a step and air above costs
# nothing, so the blast reaches further up than down.
p = triples(cases["floor_r4"]["positions"])
up = max(y for _, y, _ in p)
down = min(y for _, y, _ in p)
if (up - 65) <= (65 - down):
    sys.exit(f"floor_r4 reaches {up - 65} up and {65 - down} down — it is not "
             "asymmetric, so the floor is not being read")

print(f"EXPLOSION-RAY {len(cases)} cases, "
      f"fire in {len(fired)} of them and {len(moved)} of those move under the "
      f"shuffle; "
      f"{sum(c['count'] for c in cases.values())} positions total, "
      f"open_air_r4_s1={cases['open_air_r4_s1']['count']} "
      f"floor_r4={cases['floor_r4']['count']} (up {up - 65}, down {65 - down})")
PY

mv "$WORK/out.json" "$OUT"
echo "explosion ray -> $OUT (identical across two runs)"
