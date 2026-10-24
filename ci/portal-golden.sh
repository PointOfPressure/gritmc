#!/usr/bin/env bash
# border-golden.sh — vanilla's own world-border numbers and the six clientbound
# border packets, captured at a fixed script of operations.
#
#     ./ci/border-golden.sh [out.json]
#
# Produces `crates/rustmc-server/golden/portal_shape.json`, which is a
# **committed fixture compared against**, not a `data/vanilla/` input the
# server reads at runtime. The distinction is the one `ci/attributes-golden.sh`
# documents: a stale input silently changes behaviour, a stale fixture reddens
# a test.
#
# See `tools/goldengen/vanilla/PortalGolden.java` for what the script of
# operations is and why a default border alone would prove nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/portal_shape.json}"

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
    "$ROOT/tools/goldengen/vanilla/PortalGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx1G -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.PortalGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. Nothing in the border draws from a random
# source; a disagreement would mean the capture is reading something ambient
# and every byte below would be a coincidence.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of PortalGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
rows = {r["name"]: r for r in d["shapes"]}
print(f"portal shape -> {sys.argv[1]} (identical across two runs)")
print(f"  {len(rows)} frames, bounds {d['minWidth']}..{d['maxWidth']} wide, "
      f"{d['minHeight']}..{d['maxHeight']} tall")

fail = []
# Each of these is the property the row exists to record. A capture where they
# all came out the same way would look full and measure one frame.
expect = {
    "min-2x3-x":          (2, 3, 0, True, False),
    "min-2x3-x-lit":      (2, 3, 6, True, True),
    "min-2x3-x-half-lit": (2, 3, 2, True, False),
    "min-2x3-z":          (2, 3, 0, True, False),
    "too-narrow-1x3":     (0, 0, 0, False, False),
    "too-short-2x2":      (2, 0, 0, False, False),
    "max-21x21":          (21, 21, 0, True, False),
    "too-wide-22x3":      (0, 0, 0, False, False),
    "too-tall-2x22":      (2, 0, 0, False, False),
    "no-corners-3x4":     (3, 4, 0, True, False),
    "stone-frame-2x3":    (0, 0, 0, False, False),
    "empty-world":        (0, 0, 0, False, False),
}
for name, (w, h, n, valid, complete) in expect.items():
    r = rows.get(name)
    if r is None:
        fail.append(f"{name}: missing")
        continue
    got = (r["width"], r["height"], r["numPortalBlocks"], r["valid"], r["complete"])
    if got != (w, h, n, valid, complete):
        fail.append(f"{name}: {got} != {(w, h, n, valid, complete)}")

# The two holes must both be invalid but for different reasons: the top hole
# leaves the height walk succeeding and hasTopFrame failing, the side hole
# stops the height walk itself.
if rows["hole-in-top-3x4"]["valid"] or rows["hole-in-side-3x4"]["valid"]:
    fail.append("a frame with a hole came out valid")

# The fire row is the one that depends on a datapack TAG being bound. If tags
# are unbound in this JVM, `isEmpty` rejects fire and the frame reads as a
# 1-wide non-portal, which is a different answer from a real server's.
f = rows["fire-inside-2x3"]
if (f["width"], f["height"], f["valid"]) != (2, 3, True):
    fail.append(f"fire-inside-2x3 came out {f['width']}x{f['height']} valid={f['valid']}"
                " — BlockTags.FIRE is not bound in this JVM, so isEmpty is answering"
                " differently from a real server and this capture is not usable")

# Both axes must have produced a different rightDir, or the axis argument is
# doing nothing.
if rows["min-2x3-x"]["rightDir"] == rows["min-2x3-z"]["rightDir"]:
    fail.append("both axes gave the same rightDir")

for r in d["shapes"]:
    print(f"  {r['name']:<22} {r['width']:>3}x{r['height']:<3} portal={r['numPortalBlocks']:<4}"
          f" bl={tuple(r['bottomLeft'])} right={r['rightDir']:<6}"
          f" valid={r['valid']} complete={r['complete']}")

if fail:
    for x in fail:
        print(f"  !! {x}")
    sys.exit(1)
PYEOF
