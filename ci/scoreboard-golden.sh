#!/usr/bin/env bash
# border-golden.sh — vanilla's own world-border numbers and the six clientbound
# border packets, captured at a fixed script of operations.
#
#     ./ci/border-golden.sh [out.json]
#
# Produces `crates/rustmc-server/golden/scoreboard.json`, which is a
# **committed fixture compared against**, not a `data/vanilla/` input the
# server reads at runtime. The distinction is the one `ci/attributes-golden.sh`
# documents: a stale input silently changes behaviour, a stale fixture reddens
# a test.
#
# See `tools/goldengen/vanilla/ScoreboardGolden.java` for what the script of
# operations is and why a default border alone would prove nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/scoreboard.json}"

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
    "$ROOT/tools/goldengen/vanilla/ScoreboardGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx1G -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.ScoreboardGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. Nothing in the border draws from a random
# source; a disagreement would mean the capture is reading something ambient
# and every byte below would be a coincidence.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of ScoreboardGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
p = d["packets"]
print(f"scoreboard -> {sys.argv[1]} (identical across two runs)")
print(f"  {len(p)} captured packets, "
      f"{len(d['displaySlots'])} display slots, {len(d['teamColors'])} team colours, "
      f"{len(d['visibility'])} visibilities, {len(d['collisionRules'])} collision rules, "
      f"{len(d['renderTypes'])} render types")

fail = []
# A table that is short is a table with a hole in it, and the hole would be a
# packet field silently written as the wrong number.
for k, n in (("displaySlots", 19), ("teamColors", 16), ("visibility", 4),
             ("collisionRules", 4), ("renderTypes", 2)):
    if len(d[k]) != n:
        fail.append(f"{k}: {len(d[k])} entries, expected {n}")
    ids = sorted(d[k].values())
    if ids != list(range(len(ids))):
        fail.append(f"{k}: ids are not 0..{len(ids) - 1}: {ids}")

# Every capture distinct: a `pkt()` that silently encoded the same object
# would give a file that looks full and measures one packet.
if len(set(p.values())) != len(p):
    dupes = [k for k in p if list(p.values()).count(p[k]) > 1]
    fail.append(f"duplicate encodings: {dupes}")

# The three that must differ in a specific way, or the capture is not
# exercising the branch it is named for.
if len(p["set_objective/remove/kills"]) >= len(p["set_objective/add/kills"]):
    fail.append("set_objective REMOVE is not shorter than ADD — the `if` is not being skipped")
if len(p["set_score/negative"]) <= len(p["set_score/positive"]):
    fail.append("a negative varint score is not longer than a positive one")
if len(p["set_player_team/remove"]) >= len(p["set_player_team/create"]):
    fail.append("set_player_team REMOVE is not shorter than CREATE")

for k in sorted(p):
    print(f"  {k:<36} {len(p[k]) // 2:>4} bytes")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
