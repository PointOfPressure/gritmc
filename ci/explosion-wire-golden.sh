#!/usr/bin/env bash
# explosion-wire-golden.sh — the bytes vanilla's own codec writes for the
# `explode` clientbound packet.
#
# One JVM run of `ExplosionWireGolden`, which builds a real
# `RegistryFriendlyByteBuf` over `BuiltInRegistries` and calls
# `ClientboundExplodePacket.STREAM_CODEC.encode`. The recorded hex is what
# vanilla puts on the socket, so the field order, the varint-versus-fixed-int
# choice per field, the `Holder`'s `+1`, the `Optional` prefix and the
# `WeightedList` framing are all measured rather than read off the codec
# composition.
#
# THE CROSS-CHECKS, and each is against a source this dumper did not produce:
#
#   * `soundId` must equal `data/vanilla/protocol_ids.json`'s entry for
#     `minecraft:entity.generic.explode`, which `ci/dump-protocol-ids.sh`
#     takes from Mojang's own report by a different route.
#   * `packetId` must equal `minecraft:explode`'s `protocol_id` in
#     `oracle-work/vanilla-<ver>/generated/reports/packets.json`, which is the
#     report `crates/rustmc-proto/src/ids.rs` is transcribed from.
#   * `blockCount` on `tnt_default` must equal `open_air_r4_s1`'s count in
#     `data/vanilla/explosion_ray.json`, so the two explosion goldens agree on
#     one figure rather than each being self-consistent.
#   * `zero_blocks` and `tnt_default` must be the SAME LENGTH. That is the
#     fixed-int check with teeth: `blockCount` goes through
#     `ByteBufCodecs.INT`, four bytes whatever the value, and a varint 0 and a
#     varint 1120 differ by a byte. A port that reached for `write_varint`
#     here would pass every field-value assertion and fail this one.
#
# **Grep any diagnostic output unanchored.** `Bootstrap.bootStrap()` redirects
# System.out through log4j.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/explosion_wire.json}"

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
    "$ROOT/tools/goldengen/vanilla/ExplosionWireGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.ExplosionWireGolden 2>/dev/null > "$1"
    test -s "$1"
}

run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of ExplosionWireGolden disagree — something in it iterates a map" >&2
    exit 1
}

PACKETS="$VDIR/generated/reports/packets.json"
IDS="$ROOT/data/vanilla/protocol_ids.json"
RAY="$ROOT/data/vanilla/explosion_ray.json"

python3 - "$WORK/out.json" "$PACKETS" "$IDS" "$RAY" <<'PY'
import json, os, sys

doc = json.load(open(sys.argv[1]))
cases = {c["name"]: c for c in doc["cases"]}
expected = {"small_no_knockback", "large_with_knockback", "tnt_default", "zero_blocks",
            "empty_block_particles"}
missing = expected - set(cases)
if missing:
    sys.exit(f"cases missing: {sorted(missing)}")

for name, c in cases.items():
    if not c["bytes"]:
        sys.exit(f"{name}: the codec wrote nothing")
    if len(c["bytes"]) != c["length"] * 2:
        sys.exit(f"{name}: {len(c['bytes'])} hex chars for {c['length']} bytes")
    if c["sound"] != "minecraft:entity.generic.explode":
        sys.exit(f"{name}: sound is {c['sound']}")

# The fixed-int check. `blockCount` is four bytes whatever it holds, so two
# cases differing only in it are the same length.
if cases["tnt_default"]["length"] != cases["zero_blocks"]["length"]:
    sys.exit("tnt_default and zero_blocks differ in length — blockCount is not a fixed int")
if cases["tnt_default"]["bytes"] == cases["zero_blocks"]["bytes"]:
    sys.exit("tnt_default and zero_blocks encode identically — blockCount is not on the wire")

# The three cross-sources.
report = json.load(open(sys.argv[2]))
want_id = report["play"]["clientbound"]["minecraft:explode"]["protocol_id"]
if doc["packetId"] != want_id:
    sys.exit(f"packetId {doc['packetId']} against the report's {want_id}")

ids = json.load(open(sys.argv[3]))
want_sound = ids["sounds"]["minecraft:entity.generic.explode"]
if doc["soundId"] != want_sound:
    sys.exit(f"soundId {doc['soundId']} against protocol_ids.json's {want_sound}")

if os.path.exists(sys.argv[4]):
    ray = json.load(open(sys.argv[4]))
    rc = {c["name"]: c for c in ray["cases"]}
    want_count = rc["open_air_r4_s1"]["count"]
    if cases["tnt_default"]["blockCount"] != want_count:
        sys.exit(f"tnt_default's blockCount {cases['tnt_default']['blockCount']} against "
                 f"explosion_ray.json's open_air_r4_s1 count {want_count} — the two "
                 "explosion goldens disagree")
    crossed = f"blockCount cross-checked against explosion_ray.json at {want_count}"
else:
    crossed = "BLOCKCOUNT UNCHECKED — no data/vanilla/explosion_ray.json to compare against"

print(f"EXPLOSION-WIRE packet {doc['packetId']}, sound {doc['soundId']}, "
      f"particles {doc['particleIds']}, {len(cases)} cases, "
      f"{sum(c['length'] for c in cases.values())} bytes total; {crossed}")
PY

mv "$WORK/out.json" "$OUT"
echo "explosion wire -> $OUT (identical across two runs)"
