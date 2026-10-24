#!/usr/bin/env bash
# block-explosion-golden.sh — regenerate the block explosion-resistance table.
#
# One JVM run of `BlockExplosionGolden`, which reads
# `Block.getExplosionResistance()` off every block in the pinned jar's
# registry. That float, maxed with the fluid's, is the only thing vanilla's
# explosion ray walk asks a block
# (`ExplosionDamageCalculator.getBlockExplosionResistance`, offsets 0-42), so
# a table nobody could derive from a rule is the only honest form for it.
#
# THE CROSS-CHECK IS INSIDE THE DUMPER AND IS ASSERTED HERE, IN THREE ARMS.
#
# 1. `methodOverridesField` — every block's virtual `getExplosionResistance()`
#    against a reflective read of the `BlockBehaviour.explosionResistance`
#    field the base implementation returns. Empty in 26.2.
# 2. `stairsDisagreeingWithBase` / `stairsChecked` — the one class that does
#    override the method (`StairBlock`, returning `this.base`'s) checked
#    through a different field. Empty, over a non-zero count of stairs. This
#    arm is why arm 1's emptiness is a claim and not an absence of evidence;
#    a `stairsChecked` of zero makes arm 1 vacuous and this script refuses it.
# 3. The denominator, against a **different dumper**: `states` here must equal
#    the state count in `data/vanilla/block_props.json`, which
#    `ci/block-props-golden.sh` derives from the same registry by another
#    route. A table that has silently lost half the registry cannot pass this.
#
# The first version of this script had only arm 1 and refused itself on its
# first run, because in 26.2 the call and the field agree everywhere. That
# refusal was correct and is what arms 2 and 3 exist to answer.
#
# Same signature-stripping as the other golden scripts: the dumper lives
# inside vanilla's own package and the JVM refuses to mix an unsigned class
# into a signed one.
#
# **Grep any diagnostic output unanchored.** `Bootstrap.bootStrap()` redirects
# System.out through log4j, which re-emits it with a
# `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/block_explosion.json}"

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
    "$ROOT/tools/goldengen/vanilla/BlockExplosionGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.BlockExplosionGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. The block registry's iteration order is stable
# between JVM runs and the reflective read cannot vary, so a disagreement here
# is a bug in the dumper and not a property of the jar — but the other golden
# scripts pay for this check because the assumption has failed before, and a
# table that churns hides a real regression inside the churn.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of BlockExplosionGolden disagree — something in it iterates a map" >&2
    exit 1
}

# Arm 2 first, because it is what gives arm 1 its meaning.
STAIRS=$(grep -oP '"stairsChecked": \K[0-9]+' "$WORK/out.json")
if [ "${STAIRS:-0}" -lt 1 ]; then
    echo "stairsChecked=$STAIRS — the reflective arm found no StairBlock, so the one" >&2
    echo "class that overrides getExplosionResistance was never checked and" >&2
    echo "methodOverridesField's emptiness means nothing. Refusing." >&2
    exit 1
fi
STAIRDIS=$(grep -oP '"stairsDisagreeingWithBase": \[\K[^]]*' "$WORK/out.json")
OVERRIDES=$(grep -oP '"methodOverridesField": \[\K[^]]*' "$WORK/out.json")
if [ -n "$STAIRDIS" ] && [ -z "$OVERRIDES" ]; then
    echo "a stair's resistance differs from its base's, but methodOverridesField is" >&2
    echo "empty — the two arms have come apart, so one of them has stopped reading." >&2
    exit 1
fi
NOVR=$(printf '%s' "$OVERRIDES" | tr ',' '\n' | grep -c '"' || true)
NSTAIRDIS=$(printf '%s' "$STAIRDIS" | tr ',' '\n' | grep -c '"' || true)

# Arm 3: the denominator against a different dumper.
PROPS="$ROOT/data/vanilla/block_props.json"
if [ -s "$PROPS" ]; then
    THEIRS=$(grep -oP '"states": *\K[0-9]+' "$PROPS" | head -1)
    MINE=$(grep -oP '"states": \K[0-9]+' "$WORK/out.json")
    if [ -n "$THEIRS" ] && [ "$THEIRS" != "$MINE" ]; then
        echo "state count $MINE here against $THEIRS in data/vanilla/block_props.json —" >&2
        echo "two dumpers reading one registry do not agree on its size." >&2
        exit 1
    fi
    CROSS="cross-checked against block_props.json at $THEIRS states"
else
    # Not a skip that reads as a pass: the count is quoted as unchecked.
    CROSS="DENOMINATOR UNCHECKED — no data/vanilla/block_props.json to compare against"
fi

# The other half: keying by block is only sound while the call goes through
# the block. `stateDisagreements` counts states whose block's resistance is
# not its own block's, which is zero by construction and stops being zero the
# day the call moves onto the state.
STATEDIS=$(grep -oP '"stateDisagreements": \K[0-9]+' "$WORK/out.json")
if [ "$STATEDIS" != "0" ]; then
    echo "stateDisagreements=$STATEDIS — getExplosionResistance is no longer a" >&2
    echo "property of the block alone and this table's shape is wrong." >&2
    exit 1
fi
UNMAPPED=$(grep -oP '"unmappedStates": \K[0-9]+' "$WORK/out.json")
if [ "$UNMAPPED" != "0" ]; then
    echo "unmappedStates=$UNMAPPED — a block state resolved to no block" >&2
    exit 1
fi

BLOCKS=$(grep -oP '"blocks": \K[0-9]+' "$WORK/out.json")
STATES=$(grep -oP '"states": \K[0-9]+' "$WORK/out.json")
ROWS=$(grep -oP '"rows": \K[0-9]+' "$WORK/out.json")

mv "$WORK/out.json" "$OUT"
echo "block explosion -> $OUT ($BLOCKS blocks, $STATES states, $ROWS distinct" \
     "(resistance, dropFromExplosion) rows; $NOVR method-overrides-field," \
     "$NSTAIRDIS of $STAIRS stairs disagree with their base; $CROSS;" \
     "identical across two runs)"
