#!/usr/bin/env bash
# chunklevel-golden.sh — the ticket-level table, from the JVM.
#
# Three of `ChunkLevel`'s five constants are literals a `javap` reads off
# directly. The other two are not:
#
#     RADIUS_AROUND_FULL_CHUNK = GENERATION_PYRAMID.getStepTo(FULL)
#                                  .accumulatedDependencies().getRadius()
#     MAX_LEVEL = FULL_CHUNK_LEVEL + RADIUS_AROUND_FULL_CHUNK
#
# They fall out of every ChunkStatus's dependency radius accumulated over the
# whole pyramid, and a port that reads the bytecode of the static initialiser
# has read the *recipe*, not the number. `MAX_LEVEL` is the boundary between
# a chunk that is loaded and a chunk that is unloaded, so guessing it wrong
# moves where the world stops existing — and nothing in a value test would
# say so, because every chunk inside the guess would still be right.
#
# Same signature-stripping as `ci/hash-order-golden.sh`, for the same reason:
# the dumper lives inside vanilla's own package and the JVM refuses to mix an
# unsigned class into a signed one.
#
# The output is a committed fixture under `crates/rustmc-server/golden/`
# rather than a `data/vanilla/` table, because nothing reads it at run time —
# it gates constants that are compiled in.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/chunk_level.json}"

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
    "$ROOT/tools/goldengen/vanilla/ChunkLevelGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.server.level.ChunkLevelGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. The pyramid is built in a static initialiser
# from an ImmutableList, so this should be constant across runs — which is a
# prediction, and this is the line that tests it rather than assuming it.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of ChunkLevelGolden disagree — the pyramid is not deterministic" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
print(f"chunk levels -> {sys.argv[1]} (identical across two runs)")
print(f"  FULL={d['full_chunk_level']} BLOCK_TICKING={d['block_ticking_level']} "
      f"ENTITY_TICKING={d['entity_ticking_level']} "
      f"RADIUS_AROUND_FULL_CHUNK={d['radius_around_full_chunk']} "
      f"MAX_LEVEL={d['max_level']}")

fail = []
# The relation the port depends on. If it ever stops holding, the port's
# `MAX_LEVEL` is not `FULL + RADIUS` and this file is the only warning.
if d["max_level"] != d["full_chunk_level"] + d["radius_around_full_chunk"]:
    fail.append("max_level is not full_chunk_level + radius_around_full_chunk")
# `fullStatus` must be monotone: as the level rises the chunk can only lose
# capability, never regain it. A dump where it is not is a dump of something
# other than `ChunkLevel`.
rank = {"ENTITY_TICKING": 0, "BLOCK_TICKING": 1, "FULL": 2, "INACCESSIBLE": 3}
prev = -1
for row in d["levels"]:
    r = rank[row["full_status"]]
    if r < prev:
        fail.append(f"full_status goes backwards at level {row['level']}")
    prev = r
    if row["entity_ticking"] != (row["level"] <= d["entity_ticking_level"]):
        fail.append(f"isEntityTicking disagrees with the constant at {row['level']}")
    if row["block_ticking"] != (row["level"] <= d["block_ticking_level"]):
        fail.append(f"isBlockTicking disagrees with the constant at {row['level']}")
    if row["loaded"] != (row["level"] <= d["max_level"]):
        fail.append(f"isLoaded disagrees with MAX_LEVEL at {row['level']}")
if len(d["by_status"]) != 4:
    fail.append(f"{len(d['by_status'])} FullChunkStatus values, expected 4")

if fail:
    for f in fail:
        print(f"  !! {f}", file=sys.stderr)
    sys.exit(1)
print(f"  {len(d['levels'])} levels dumped, {len(d['by_status'])} statuses, all self-consistent")
PYEOF
