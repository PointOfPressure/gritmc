#!/usr/bin/env bash
# border-golden.sh — vanilla's own world-border numbers and the six clientbound
# border packets, captured at a fixed script of operations.
#
#     ./ci/border-golden.sh [out.json]
#
# Produces `crates/rustmc-server/golden/villager_professions.json`, which is a
# **committed fixture compared against**, not a `data/vanilla/` input the
# server reads at runtime. The distinction is the one `ci/attributes-golden.sh`
# documents: a stale input silently changes behaviour, a stale fixture reddens
# a test.
#
# See `tools/goldengen/vanilla/VillagerGolden.java` for what the script of
# operations is and why a default border alone would prove nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/villager_professions.json}"

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
    "$ROOT/tools/goldengen/vanilla/VillagerGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx1G -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.VillagerGolden "$1" >/dev/null 2>&1
    test -s "$1"
}

# Twice, and they must agree. Nothing in the border draws from a random
# source; a disagreement would mean the capture is reading something ambient
# and every byte below would be a coincidence.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of VillagerGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
poi = {r["name"]: r for r in d["poiTypes"]}
prof = {r["name"]: r for r in d["professions"]}
print(f"villager professions -> {sys.argv[1]} (identical across two runs)")
print(f"  {len(poi)} POI types, {len(prof)} professions")

fail = []
if len(poi) != 21:
    fail.append(f"{len(poi)} POI types, expected 21")
if len(prof) != 15:
    fail.append(f"{len(prof)} professions, expected 15")
# The two professions with no trade sets at all. `nitwit` and `none` are the
# reason the datapack has 14 profession directories and the registry 15
# entries plus none; if either grew a trade set the join below would change
# meaning.
for name in ("minecraft:nitwit", "minecraft:none"):
    r = prof.get(name)
    if r is not None and r["tradeSetsByLevel"]:
        fail.append(f"{name} has trade sets: {r['tradeSetsByLevel']}")
# Every trade set a profession names must be five levels, or the join to the
# datapack is partial and a level would silently deal nothing.
for name, r in prof.items():
    sets = r["tradeSetsByLevel"]
    if sets and sorted(sets) != ["1", "2", "3", "4", "5"]:
        fail.append(f"{name} has levels {sorted(sets)}")
# maxTickets separates a job site from a meeting point from a marker, and a
# table where every value were equal would read nothing.
tickets = {r["maxTickets"] for r in poi.values()}
if len(tickets) < 3:
    fail.append(f"only {len(tickets)} distinct maxTickets values: {sorted(tickets)}")
ranges = {r["validRange"] for r in poi.values()}
if len(ranges) < 2:
    fail.append(f"only {len(ranges)} distinct validRange values: {sorted(ranges)}")
# A POI whose state set is empty matches nothing and would be a dead row.
for name, r in poi.items():
    if r["states"] == 0:
        fail.append(f"{name} matches no block states")

for name in sorted(poi):
    r = poi[name]
    print(f"  poi  {name:<42} tickets={r['maxTickets']} range={r['validRange']:>2} "
          f"states={r['states']:>4} {','.join(r['blocks'])}")
for name in sorted(prof):
    r = prof[name]
    print(f"  prof {name:<28} sound={r['workSound']} "
          f"job={','.join(r['acquirableJobSite']) or '-'} "
          f"levels={len(r['tradeSetsByLevel'])} "
          f"wants={len(r['requestedItems'])} secondary={len(r['secondaryPoi'])}")

if fail:
    for x in fail:
        print(f"  !! {x}")
    sys.exit(1)
PYEOF
