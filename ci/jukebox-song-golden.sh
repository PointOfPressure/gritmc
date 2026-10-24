#!/usr/bin/env bash
# jukebox-song-golden.sh — the JUKEBOX_SONG registry.
#
#     ./ci/jukebox-song-golden.sh [out.json]
#
# Produces `data/vanilla/jukebox_songs.json`: every jukebox song with its
# length in ticks, its comparator output, and **two** candidate registry
# indices that do not agree with each other. See
# `tools/goldengen/vanilla/JukeboxSongGolden.java` for why there are two and
# why neither is called "the id".
#
# **A runtime extract, not a fixture**: `data/vanilla/`, gitignored,
# regenerated per tree by `ci/gen-gameplay-tables.sh`, stamped by
# `ci/provenance.sh`.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/jukebox_songs.json}"

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
    "$ROOT/tools/goldengen/vanilla/JukeboxSongGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" \
        -Drustmc.tags.dir="$VDIR/generated-data/data/minecraft/tags" \
        -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.JukeboxSongGolden 2>"$WORK/err" > "$1"
    # stderr to a file rather than to /dev/null, so a bootstrap failure is a
    # stack trace on the terminal rather than a bare `set -e` exit.
    test -s "$1" || { sed -n "1,20p" "$WORK/err" >&2; return 1; }
}

# Twice, and they must agree. The bootstrap order is a property of a static
# initialiser and ought to be stable; this is the check that says so.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of JukeboxSongGolden disagree — the registry order is not stable" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
table = d["table"]
print(f"jukebox songs -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['songs']} songs, rows sorted by id; the registry's own order is "
      f"not reproducible and carries no column here")

fail = []
if len(table) != d["songs"]:
    fail.append(f"{len(table)} rows for {d['songs']} songs")

# The rows must be sorted by id and the rank must be the row number. That is
# what makes the file byte-reproducible despite the registry's own order not
# being — see the dumper's header for the three-run measurement.
if [r["song"] for r in table] != sorted(r["song"] for r in table):
    fail.append("rows are not sorted by song id")
if [r["alphabeticalRank"] for r in table] != list(range(len(table))):
    fail.append("alphabeticalRank is not the row number")
if not d.get("registryIdIsUnverified"):
    fail.append("the file no longer flags its rank column as unverified")

# The level event ids, which are literals in JukeboxSongPlayer and not
# derived from anything.
if d["playLevelEvent"] != 1010 or d["stopLevelEvent"] != 1011:
    fail.append(f"level events read {d['playLevelEvent']}/{d['stopLevelEvent']}, expected 1010/1011")
if d["finishedTailTicks"] != 20:
    fail.append(f"the hasFinished tail is {d['finishedTailTicks']}, expected 20")

# A collapsed table looks fine until something reads a length. These four are
# pinned against the datapack JSON, which is a different file produced by a
# different tool: 13 is 178.0 s -> 3560 ticks, and its comparator output is 1.
want = {
    "minecraft:13": (3560, 1),
    "minecraft:11": (1420, 11),
    "minecraft:pigstep": (2980, 13),
    "minecraft:blocks": (6900, 3),
}
have = {r["song"]: (r["lengthInTicks"], r["comparatorOutput"]) for r in table}
for song, pair in want.items():
    if song not in have:
        fail.append(f"{song} is not in the table")
    elif have[song] != pair:
        fail.append(f"{song} reads {have[song]}, expected {pair}")

# Comparator outputs run 1..15 and a table that collapsed to one value would
# make every disc read the same on a comparator.
outs = {r["comparatorOutput"] for r in table}
if len(outs) < 10:
    fail.append(f"only {len(outs)} distinct comparator outputs — the table has collapsed")
if min(outs) < 1 or max(outs) > 15:
    fail.append(f"comparator outputs run {min(outs)}..{max(outs)}, expected 1..15")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
