#!/usr/bin/env bash
# sound-types-golden.sh — the `SoundType` behind every block state.
#
#     ./ci/sound-types-golden.sh [out.json]
#
# Produces `data/vanilla/sound_types.json`: one row per distinct `SoundType`
# instance — volume, pitch and the five events — plus a state-indexed array of
# row numbers. See `tools/goldengen/vanilla/SoundTypeGolden.java` for the three
# call sites that read it and for why it is keyed by state rather than block.
#
# **A runtime extract, not a fixture**: `data/vanilla/`, gitignored,
# regenerated per tree by `ci/gen-gameplay-tables.sh`, stamped by
# `ci/provenance.sh`. A stale one makes a placed block sound like the wrong
# material, which is the kind of wrong nobody notices in a log.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/sound_types.json}"

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
    "$ROOT/tools/goldengen/vanilla/SoundTypeGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.SoundTypeGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. `LinkedHashMap` over the state registry gives a
# stable row order only if that registry iterates stably; this is the check
# that says so rather than the assumption that it does.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of SoundTypeGolden disagree — the row order is not stable" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, struct, sys

d = json.load(open(sys.argv[1]))
print(f"sound types -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['soundTypes']} distinct SoundTypes over {d['states']} states; "
      f"{d['overridingBlocks']} blocks have a state that overrides their default")

fail = []
if len(d["table"]) != d["soundTypes"]:
    fail.append(f"{len(d['table'])} rows for {d['soundTypes']} sound types")
if len(d["stateTypes"]) != d["states"]:
    fail.append(f"{len(d['stateTypes'])} state entries for {d['states']} states")
# Every block state has a sound type — the field is set from `Properties` and
# is never null. A `-1` can only mean the dumper missed a state, and a reader
# that treated it as "silent" would make that block place without a sound.
if d["unmappedStates"] != 0:
    fail.append(f"{d['unmappedStates']} states map to no sound type")
# One row would mean the identity map collapsed and every block sounds the
# same, which is a table that looks fine and is entirely wrong.
if d["soundTypes"] < 20:
    fail.append(f"only {d['soundTypes']} distinct sound types — the identity keying has collapsed")

rows = d["table"]

# The four rows the corpus actually exercises, pinned against numbers that
# came from somewhere other than this dumper:
#
#   grass  — 080's step sound is `block.grass.step` at volume 0.15, which is
#            `getVolume() * 0.15F`, so grass volume must be 1.0 and its pitch
#            1.0. **Both are compared as of 2026-08-17**: `block.*.step` and
#            `block.*.place` are `Constant` in `tools/oracle-runner/src/pitch.rs`,
#            because `Entity.playStepSound` and `BlockItem.place` pass
#            `SoundType.getPitch()` without touching a `RandomSource`. This
#            line used to read "the recorded pitch field is not compared, but a
#            wrong pitch here would also give the wrong PLACE pitch, which is" —
#            the indirection is no longer needed, and a wrong pitch in this
#            table now fails a gameplay scenario directly.
#   stone  — the recording shows a placed stone at volume 1.0 pitch 0.8, and
#            the place arithmetic is `(v+1)/2` and `p*0.8`, so stone is 1.0/1.0.
#   metal  — a placed rail sounds `block.metal.place` at pitch 1.2, which is
#            METAL's 1.5 times 0.8.
#   wood   — the fourth family the corpus places, included so that a table
#            right on three rows and wrong elsewhere still fails.
#
# The dumper does not emit block names, so the rows are found by their event
# ids, which is the identity that matters here.
def find(place_event):
    hits = [r for r in rows if r["place"] == place_event]
    return hits[0] if hits else None

for place_event, vol, pitch, step_event in (
    ("minecraft:block.grass.place", 1.0, 1.0, "minecraft:block.grass.step"),
    ("minecraft:block.stone.place", 1.0, 1.0, "minecraft:block.stone.step"),
    ("minecraft:block.metal.place", 1.0, 1.5, "minecraft:block.metal.step"),
    ("minecraft:block.wood.place", 1.0, 1.0, "minecraft:block.wood.step"),
):
    r = find(place_event)
    if r is None:
        fail.append(f"no sound type places {place_event}")
        continue
    # Both sides through a float32 round trip: these are Java floats printed
    # by `Float.toString`, and `0.6 != float32(0.6)` as a double.
    for field, want in (("volume", vol), ("pitch", pitch)):
        got32 = struct.unpack("<f", struct.pack("<f", r[field]))[0]
        if got32 != struct.unpack("<f", struct.pack("<f", want))[0]:
            fail.append(f"{place_event}: {field} is {r[field]}, expected {want}")
        bits = r[field + "Bits"] & 0xFFFFFFFF
        if struct.unpack("<f", struct.pack("<I", bits))[0] != got32:
            fail.append(f"{place_event}: {field} bits and value disagree")
    if r["step"] != step_event:
        fail.append(f"{place_event}: step is {r['step']}, expected {step_event}")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
