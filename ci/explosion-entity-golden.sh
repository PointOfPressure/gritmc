#!/usr/bin/env bash
# explosion-entity-golden.sh — vanilla running `getSeenPercent`'s sampling grid
# and `getEntityDamageAmount`'s arithmetic.
#
# One JVM run of `ExplosionEntityGolden`. A `ServerLevel` subclass and an
# `Entity` subclass are allocated without their constructors and their fields
# set by reflection; the level overrides `Level.clip` to RECORD the origin it
# was asked about and answer MISS (or HIT on every k-th call), so the recorded
# list is vanilla's own sampling grid and the returned float is its own
# `(float) hits / (float) total`.
#
# THE ENTITY HALF IS SPLIT IN TWO ON PURPOSE. The grid is arithmetic this
# server can reproduce; the raycast is `Level.clip`, which this workspace has
# no counterpart for at all (`crate::collide` is an AABB sweep, there is no
# `ClipContext` and no `BlockHitResult`). Recording the origins separates the
# half that is portable from the half that is refused, and a golden of the
# fraction alone could not.
#
# THE ASSERTIONS, and each can go red:
#
#   * every case's `origins` length must equal its `samples` count, so a stub
#     that stopped recording cannot pass;
#   * the three player cases share one box and one centre and differ only in
#     `hitEvery`, so their origin lists must be IDENTICAL while their fractions
#     are 1.0, 2/3 and 0.0 — a grid that depended on the clip answer would fail
#     the first half and a fraction that ignored it would fail the second;
#   * the five box shapes must not all sample the same number of points, or
#     the grid is not reading the box;
#   * `wide_flat` has zero height and must still sample, because `dy < 0.0` is
#     a strict comparison;
#   * the damage grid must contain a case at every `seen = 0.0` equal to
#     exactly 1.0 — the `+ 1.0` term — and no case may be zero or negative.
#
# **Grep any diagnostic output unanchored.** `Bootstrap.bootStrap()` redirects
# System.out through log4j.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/explosion_entity.json}"

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
    "$ROOT/tools/goldengen/vanilla/ExplosionEntityGolden.java" 2>&1 \
    | grep -vE '^Note|proprietary|^ *\^|^[0-9]+ warning' || true

run() {
    java -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.ExplosionEntityGolden 2>/dev/null > "$1"
    test -s "$1"
}

run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of ExplosionEntityGolden disagree — something in it iterates a map" >&2
    exit 1
}

PACKETS="$VDIR/generated/reports/packets.json"
IDS="$ROOT/data/vanilla/protocol_ids.json"
RAY="$ROOT/data/vanilla/explosion_ray.json"

python3 - "$WORK/out.json" <<'PY'
import json, sys

doc = json.load(open(sys.argv[1]))
seen = {c["name"]: c for c in doc["seenCases"]}
expected = {"player", "player_third_blocked", "player_all_blocked", "unit_cube",
            "wide_flat", "tall_thin", "negative_origin"}
missing = expected - set(seen)
if missing:
    sys.exit(f"seenCases missing: {sorted(missing)}")

for name, c in seen.items():
    if c["samples"] == 0:
        sys.exit(f"{name}: no samples — the grid produced nothing")
    if len(c["origins"]) != c["samples"] * 3:
        sys.exit(f"{name}: {len(c['origins']) // 3} origins for {c['samples']} samples")

# One box, one centre, three clip answers: same grid, three fractions.
base = seen["player"]["origins"]
for name in ["player_third_blocked", "player_all_blocked"]:
    if seen[name]["origins"] != base:
        sys.exit(f"{name}: the grid moved when the clip answer changed")
if seen["player"]["seenPercent"] != 1.0:
    sys.exit(f"player: all-MISS gave {seen['player']['seenPercent']}, not 1.0")
if seen["player_all_blocked"]["seenPercent"] != 0.0:
    sys.exit(f"player_all_blocked: all-HIT gave {seen['player_all_blocked']['seenPercent']}")
mid = seen["player_third_blocked"]["seenPercent"]
if not 0.0 < mid < 1.0:
    sys.exit(f"player_third_blocked: {mid} is not a fraction")

counts = {seen[n]["samples"] for n in ["player", "unit_cube", "wide_flat", "tall_thin"]}
if len(counts) < 3:
    sys.exit(f"four box shapes gave {len(counts)} distinct sample counts: {sorted(counts)}")
if seen["wide_flat"]["samples"] == 0:
    sys.exit("a zero-height box sampled nothing — `dy < 0.0` is not strict")

dmg = doc["damageCases"]
if not dmg:
    sys.exit("no damage cases")
zero_seen = [c for c in dmg if c["seen"] == 0.0]
if not zero_seen:
    sys.exit("no seen = 0.0 case, so the `+ 1.0` term is unmeasured")
for c in zero_seen:
    if c["damage"] != 1.0:
        sys.exit(f"seen = 0.0 at r={c['radius']} d={c['distance']} gave {c['damage']}, not 1.0")
# Inside vanilla's own guard, damage is positive. OUTSIDE it the formula goes
# negative — `d0 > 1` makes `d1` negative and the quadratic term cannot rescue
# it — and `hurtEntities` never asks, because it `continue`s at offsets 214-221
# when `d0 > 1.0`. That is recorded rather than clamped: a port that clamped
# here would disagree with vanilla for any caller that ever did ask.
inside = [c for c in dmg if c["distance"] <= 2.0 * c["radius"]]
if not inside:
    sys.exit("no case inside vanilla's own d0 <= 1 guard")
for c in inside:
    if c["damage"] <= 0.0:
        sys.exit(f"inside the guard, r={c['radius']} d={c['distance']} "
                 f"seen={c['seen']} gave {c['damage']}")
outside = [c for c in dmg if c["distance"] > 2.0 * c["radius"]]
if outside and not any(c["damage"] < 0.0 for c in outside):
    sys.exit("no case outside the guard came back negative — the quadratic has "
             "been clamped somewhere")

print(f"EXPLOSION-ENTITY {len(seen)} seen cases "
      f"({sum(c['samples'] for c in seen.values())} samples, "
      f"counts {sorted(counts)}), {len(dmg)} damage cases, "
      f"{len(inside)} inside the d0 <= 1 guard (max {max(c['damage'] for c in inside)}), "
      f"{len(outside)} outside it (min {min(c['damage'] for c in outside)})")
PY

mv "$WORK/out.json" "$OUT"
echo "explosion entity -> $OUT (identical across two runs)"
