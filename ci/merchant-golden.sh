#!/usr/bin/env bash
# border-golden.sh — vanilla's own world-border numbers and the six clientbound
# border packets, captured at a fixed script of operations.
#
#     ./ci/border-golden.sh [out.json]
#
# Produces `crates/rustmc-server/golden/merchant_offers.json`, which is a
# **committed fixture compared against**, not a `data/vanilla/` input the
# server reads at runtime. The distinction is the one `ci/attributes-golden.sh`
# documents: a stale input silently changes behaviour, a stale fixture reddens
# a test.
#
# See `tools/goldengen/vanilla/MerchantGolden.java` for what the script of
# operations is and why a default border alone would prove nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/crates/rustmc-server/golden/merchant_offers.json}"

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
    "$ROOT/tools/goldengen/vanilla/MerchantGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx1G -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.MerchantGolden "$1" >/dev/null 2>&1
    test -s "$1"
}

# Twice, and they must agree. Nothing in the border draws from a random
# source; a disagreement would mean the capture is reading something ambient
# and every byte below would be a coincidence.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of MerchantGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
print(f"merchant offers -> {sys.argv[1]} (identical across two runs)")
print(f"  {len(d['offers'])} offers, packet {len(d['packet']) // 2} bytes, "
      f"empty packet {len(d['emptyPacket']) // 2} bytes")

fail = []
o = {r["name"]: r for r in d["offers"]}
# Each of these is a field that would be constant across the capture without
# the offer that carries it, and a constant field is one no comparison reads.
if not any(r["outOfStock"] for r in d["offers"]):
    fail.append("no offer is out of stock")
if not any(r["hasCostB"] for r in d["offers"]):
    fail.append("no offer has a second cost")
if not any(not r["hasCostB"] for r in d["offers"]):
    fail.append("every offer has a second cost")
if not any(r["specialPriceDiff"] < 0 for r in d["offers"]):
    fail.append("no offer has a negative specialPriceDiff — the signed int is untested")
if not any(r["demand"] != 0 for r in d["offers"]):
    fail.append("every offer has zero demand")
if len({r["priceMultiplier"] for r in d["offers"]}) < 2:
    fail.append("one distinct price multiplier")
if len({r["resultCount"] for r in d["offers"]}) < 2:
    fail.append("one distinct result count")

# The two-byte varints: containerId 200 and villagerXp 150 are both above 127,
# so a fixed-width read of either is wrong on this capture and right on a
# smaller one.
if d["containerId"] <= 127 or d["villagerXp"] <= 127:
    fail.append("containerId and villagerXp must both exceed 127 or the varints are untested")

# The empty packet must be shorter by more than the offers alone, and its
# offer count must be one zero byte.
if len(d["emptyPacket"]) >= len(d["packet"]):
    fail.append("the empty packet is not shorter")

for r in d["offers"]:
    print(f"  {r['name']:<30} uses={r['uses']}/{r['maxUses']} xp={r['xp']} "
          f"diff={r['specialPriceDiff']} mult={r['priceMultiplier']} demand={r['demand']} "
          f"costB={r['hasCostB']} out={r['outOfStock']}")

if fail:
    for x in fail:
        print(f"  !! {x}")
    sys.exit(1)
PYEOF
