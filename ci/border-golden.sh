#!/usr/bin/env bash
# border-golden.sh — vanilla's own world-border numbers and the six clientbound
# border packets, captured at a fixed script of operations.
#
#     ./ci/border-golden.sh [out.json]
#
# Produces `crates/rustmc-server/golden/world_border.json`, which is a
# **committed fixture compared against**, not a `data/vanilla/` input the
# server reads at runtime. The distinction is the one `ci/attributes-golden.sh`
# documents: a stale input silently changes behaviour, a stale fixture reddens
# a test.
#
# See `tools/goldengen/vanilla/BorderGolden.java` for what the script of
# operations is and why a default border alone would prove nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/world_border.json}"

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
    "$ROOT/tools/goldengen/vanilla/BorderGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx1G -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.BorderGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. Nothing in the border draws from a random
# source; a disagreement would mean the capture is reading something ambient
# and every byte below would be a coincidence.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of BorderGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
states = d["states"]
print(f"world border -> {sys.argv[1]} (identical across two runs)")
print(f"  {len(states)} states, {sum(len(s['packets']) for s in states)} captured packets")

fail = []
# The capture is only worth anything if the script actually left the default
# state. Each of these is a property the default border does NOT have, so a
# script that silently stopped moving fails here rather than passing quietly.
if not any(s["centerX"] != 0.0 for s in states):
    fail.append("no state has a non-zero centre — the capture never left the origin")
if not any(s["status"] == "SHRINKING" for s in states):
    fail.append("no SHRINKING state")
if not any(s["status"] == "GROWING" for s in states):
    fail.append("no GROWING state")
if not any(s["lerpTime"] > 0 for s in states):
    fail.append("no state is mid-lerp")
# The clamp state: size and box width must disagree, or the clamp was not hit.
clamped = [s for s in states if s["name"].startswith("clamped")]
if not clamped:
    fail.append("no clamped state")
else:
    c = clamped[0]
    if c["maxX"] - c["minX"] >= c["size"]:
        fail.append(f"clamped state box {c['maxX'] - c['minX']} is not narrower than size {c['size']}")
# Distinct bytes per packet kind, or the capture is one state repeated.
for kind in states[0]["packets"]:
    seen = {s["packets"][kind] for s in states}
    if len(seen) < 2:
        fail.append(f"{kind}: one distinct encoding across {len(states)} states")

for s in states:
    print(f"  {s['name']:<32} size={s['size']:<20} status={s['status']:<10} lerpTime={s['lerpTime']}")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
