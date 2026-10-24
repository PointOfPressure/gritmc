#!/usr/bin/env bash
# mob-effects-golden.sh — dump the whole `minecraft:mob_effect` registry from
# the jar, with the tick rule PROBED rather than read.
#
#     ./ci/mob-effects-golden.sh [out.json]
#
# Produces `data/vanilla/mob_effects.json`. See
# `tools/goldengen/vanilla/MobEffectsGolden.java` for what is in it and for the
# split it exists to enforce: forty rows of per-effect constants are data, and
# `shouldApplyEffectTickThisTick` is a program that `crates/rustmc-server/src/
# effects.rs` ports and this table's `tick` probe checks.
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
OUT="${1:-$ROOT/data/vanilla/mob_effects.json}"

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
    "$ROOT/tools/goldengen/vanilla/MobEffectsGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.MobEffectsGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. Nothing here draws from a random source, so a
# disagreement means the dump is reading something ambient — a registry
# iteration order that is not stable, or a modifier map whose iteration order
# moves — and a table whose row order moves is a table whose diff is unreadable.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of MobEffectsGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, struct, sys

d = json.load(open(sys.argv[1]))
print(f"mob effects -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['effects']} effects, {d['instantaneous']} instantaneous, "
      f"{d['beneficial']} beneficial, {d['withAttributeModifiers']} with attribute modifiers")
print(f"  {d['everTicks']} ever reach applyEffectTick "
      f"(probed to duration {d['maxProbedDuration']}, amplifier {d['maxProbedAmplifier']})")

fail = []
if len(d["table"]) != d["effects"]:
    fail.append(f"{len(d['table'])} rows for {d['effects']} effects")

by_id = {r["id"]: r for r in d["table"]}

# The four rows the port's TickRule enum is built around. Named explicitly,
# because a dump that silently lost one would leave `effects.rs` checked
# against a table with nothing in it to disagree with.
for want in ("minecraft:regeneration", "minecraft:poison", "minecraft:wither",
             "minecraft:instant_health"):
    if want not in by_id:
        fail.append(f"no {want} row")

# The claim the whole probe exists to make falsifiable: MOST effects never
# tick. If every effect ticked, `effects.rs`'s Never variant would be dead and
# the table would be agreeing with a port that had stopped saying anything.
if d["everTicks"] >= d["effects"]:
    fail.append(f"everTicks {d['everTicks']} == effects {d['effects']} — "
                "the base rule returns false, so this cannot be right")
if d["everTicks"] == 0:
    fail.append("no effect ticks at all — the probe measured nothing")

# The masked-shift case. `25 >> 32` is 25 in Java, so poison at amplifier 32
# must fire on exactly the multiples of 25 and not on every tick.
p = by_id.get("minecraft:poison")
if p is not None:
    probes = {t["amplifier"]: t["firesAt"] for t in p["tick"]}
    masked = probes.get(d["maskedShiftAmplifier"])
    at_zero = probes.get(0)
    if masked != at_zero:
        fail.append(f"poison at amplifier {d['maskedShiftAmplifier']} "
                    f"({masked}) differs from amplifier 0 ({at_zero}) — "
                    "Java's shift mask is not being exercised as expected")
    high = probes.get(d["maxProbedAmplifier"])
    if high is None or len(high) != d["maxProbedDuration"] + 1:
        fail.append("poison at the top probed amplifier does not fire every tick — "
                    "the shift-to-zero arm is not covered")

# A bit pattern that does not decode to the readable value means the two
# fields disagree and a reader taking the bits would use a different number
# from the one printed here.
for row in d["table"]:
    for group in row["attributeModifiers"]:
        for mod in group["modifiers"]:
            bits = mod["amountBits"] & 0xFFFFFFFFFFFFFFFF
            if struct.unpack("<d", struct.pack("<Q", bits))[0] != mod["amount"]:
                fail.append(f"{row['id']}: amount bits and value disagree")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
