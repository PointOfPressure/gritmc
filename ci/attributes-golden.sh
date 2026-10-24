#!/usr/bin/env bash
# attributes-golden.sh — dump the attribute registry and every entity type's
# base map from the jar.
#
#     ./ci/attributes-golden.sh [out.json]
#
# Produces `data/vanilla/attributes.json`. See
# `tools/goldengen/vanilla/AttributesGolden.java` for what is in it and why a
# constant would not do: `LivingEntity.getDefaultGravity()` is
# `getAttributeValue(Attributes.GRAVITY)`, an attribute read with a range and a
# modifier list, while `ItemEntity` and `FallingBlockEntity` each return a
# literal `0.04d`. Two of the three entity kinds this server simulates do not
# touch the attribute system at all, and the one that does must not be given a
# transcribed `0.08`.
#
# **This is a runtime extract, not a fixture.** It goes under `data/vanilla/`,
# it is gitignored, it is regenerated per tree by `ci/gen-gameplay-tables.sh`
# and stamped by `ci/provenance.sh` — the same distinction
# `ci/collision-shapes-golden.sh` documents: this file is an *input* the server
# reads, so a stale one silently changes how fast things fall, whereas a golden
# under `crates/*/golden/` is only ever compared against.
#
# Same signature-stripping as `ci/collision-shapes-golden.sh`, for the same
# reason: the dumper lives inside vanilla's own package and the JVM refuses to
# mix an unsigned class into a signed one.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/attributes.json}"

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
    "$ROOT/tools/goldengen/vanilla/AttributesGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.AttributesGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. Nothing here draws from a random source, so a
# disagreement means the dump is reading something ambient — a registry
# iteration order that is not stable, most likely — and a table whose row
# order moves is a table whose diff is unreadable.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of AttributesGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
print(f"attributes -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['attributes']} attributes, "
      f"{d['typesWithSupplier']} entity types with a base map, "
      f"{d['typesWithoutSupplier']} without")

by_id = {r["id"]: r for r in d["table"]}
fail = []
if len(d["table"]) != d["attributes"]:
    fail.append(f"{len(d['table'])} rows for {d['attributes']} attributes")

# The row this whole table exists for. Named explicitly, because a dump that
# silently lost it would leave `attributes.rs` falling back to a default and
# every living entity would weigh nothing.
g = by_id.get("minecraft:gravity")
if g is None:
    fail.append("no minecraft:gravity row — the one attribute the movement tick reads")
else:
    print(f"  minecraft:gravity default={g['default']} "
          f"range=[{g.get('min')}, {g.get('max')}] ranged={g['ranged']}")
    if not g["ranged"]:
        fail.append("minecraft:gravity is not a RangedAttribute — sanitizeValue would not clamp")
    # A bit pattern that does not decode to the readable value means the two
    # fields disagree and the reader (which takes the bits) would silently use
    # a different number from the one printed here.
    import struct
    for k in ("default", "min", "max"):
        if k in g:
            bits = g[k + "Bits"] & 0xFFFFFFFFFFFFFFFF
            if struct.unpack("<d", struct.pack("<Q", bits))[0] != g[k]:
                fail.append(f"gravity {k}: bits and value disagree")

# A player is the only kind in this corpus whose gravity is an attribute read.
# If the supplier list has no player row, `attributes.rs` has nothing to build
# a player's map from.
players = [s for s in d["suppliers"] if s["type"] == "minecraft:player"]
if not players:
    fail.append("no minecraft:player supplier row")
else:
    ids = {b["id"] for b in players[0]["base"]}
    if "minecraft:gravity" not in ids:
        fail.append("the player's base map has no minecraft:gravity")
    print(f"  minecraft:player carries {len(ids)} attributes")

# The two kinds that must NOT have one. This is the assertion that keeps
# `attributes.rs` honest about which entities the attribute path applies to:
# if an item ever grew a supplier, a gravity of 0.04 read from a literal would
# be the wrong answer and nothing else would say so.
for t in ("minecraft:item", "minecraft:falling_block"):
    if any(s["type"] == t for s in d["suppliers"]):
        fail.append(f"{t} has an AttributeSupplier — its gravity is not a literal after all")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
