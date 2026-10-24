#!/usr/bin/env bash
# potions-golden.sh — dump the whole `minecraft:potion` registry from the jar.
#
#     ./ci/potions-golden.sh [out.json]
#
# Produces `data/vanilla/potions.json`. See
# `tools/goldengen/vanilla/PotionsGolden.java` for why this one is entirely a
# table with no program beside it: `Potion` has one field and three
# non-branching methods, so every potion is forty-six rows of
# (effect, duration, amplifier) written into `Potions.<clinit>`.
#
# **This is a runtime extract, not a fixture.** It goes under `data/vanilla/`,
# it is gitignored, it is regenerated per tree by `ci/gen-gameplay-tables.sh`
# and stamped by `ci/provenance.sh` — the same distinction
# `ci/attributes-golden.sh` documents.
#
# Same signature-stripping as `ci/attributes-golden.sh`, for the same reason:
# the dumper lives inside vanilla's own package and the JVM refuses to mix an
# unsigned class into a signed one.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/potions.json}"

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
    "$ROOT/tools/goldengen/vanilla/PotionsGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.PotionsGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. Nothing here draws from a random source, so a
# disagreement means the dump is reading something ambient — a registry
# iteration order that is not stable, or a modifier map whose iteration order
# moves — and a table whose row order moves is a table whose diff is unreadable.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of PotionsGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
print(f"potions -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['potions']} potions, {d['withInstantEffects']} with instant effects, "
      f"{d['withNoEffects']} with none, {d['effectInstances']} effect instances in total")

fail = []
if len(d["table"]) != d["potions"]:
    fail.append(f"{len(d['table'])} rows for {d['potions']} potions")

by_id = {r["id"]: r for r in d["table"]}

# `minecraft:water` is the one potion that legitimately carries no effects,
# and it is the row that proves an empty `effects` list is real data rather
# than a dump that failed to read the field.
if "minecraft:water" not in by_id:
    fail.append("no minecraft:water row")
elif by_id["minecraft:water"]["effects"]:
    fail.append("minecraft:water has effects — the empty case is not what it looks like")

# The three strengths of one potion must differ, or the dump is reading a
# constant. `swiftness` (8:00), `long_swiftness` (0:20 more per level... no:
# 8:00 -> 20:00) and `strong_swiftness` (amplifier 1) are the canonical trio.
trio = ["minecraft:swiftness", "minecraft:long_swiftness", "minecraft:strong_swiftness"]
if all(t in by_id for t in trio):
    d0 = by_id[trio[0]]["effects"][0]
    d1 = by_id[trio[1]]["effects"][0]
    d2 = by_id[trio[2]]["effects"][0]
    print(f"  swiftness {d0['duration']}t amp {d0['amplifier']}, "
          f"long {d1['duration']}t amp {d1['amplifier']}, "
          f"strong {d2['duration']}t amp {d2['amplifier']}")
    if d1["duration"] <= d0["duration"]:
        fail.append("long_swiftness is not longer than swiftness")
    if d2["amplifier"] <= d0["amplifier"]:
        fail.append("strong_swiftness is not stronger than swiftness")
    # `name()` is the translation stem and is shared by the trio.
    names = {by_id[t]["name"] for t in trio}
    if len(names) != 1:
        fail.append(f"the swiftness trio does not share a name(): {names}")
else:
    fail.append("the swiftness trio is not all present")

# An instant potion must be flagged as one, and a duration potion must not.
if by_id.get("minecraft:healing", {}).get("hasInstantEffects") is not True:
    fail.append("minecraft:healing is not flagged as having instant effects")
if by_id.get("minecraft:swiftness", {}).get("hasInstantEffects") is not False:
    fail.append("minecraft:swiftness IS flagged as having instant effects")

if d["effectInstances"] == 0:
    fail.append("no effect instances at all — the dump read nothing")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
