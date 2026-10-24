#!/usr/bin/env bash
# recipe-packet-order-probe.sh — is `update_recipes` byte-reproducible?
#
# THE QUESTION AND WHY IT IS NOT RHETORICAL
#
# The usual gate for a clientbound packet is a captured reference: boot vanilla,
# record the bytes, require ours to match. For `ClientboundUpdateRecipesPacket`
# that gate cannot exist, and the reason is measurable rather than arguable.
#
# The packet's first field is
# `Map<ResourceKey<RecipePropertySet>, RecipePropertySet>`, written by
# `ByteBufCodecs.map`, which walks the map's ITERATION order. That map is built
# in `RecipeManager.finalizeRecipeLoading` with `Collectors.toUnmodifiableMap`,
# whose result is a `java.util.ImmutableCollections` map — and those probe from
# a per-JVM-run `SALT`. Two boots of unmodified vanilla therefore send the same
# seven property sets in a different order, and a "byte-identical" gate against
# one capture would be pinning one JVM's hash salt and calling it parity.
#
# This script measures that rather than asserting it: it prints the iteration
# order of `RecipeManager.RECIPE_PROPERTY_SETS` and of the
# `toUnmodifiableMap` copy `finalizeRecipeLoading` makes of it, once per JVM
# run, and reports how many DISTINCT orders N runs produced.
#
# HOW TO READ THE RESULT
#
#   distinct > 1   the packet's wire order is a property of the JVM, not of the
#                  server. Gate the CONTENT (`property_sets` in
#                  `data/vanilla/recipe_match.json`), never the bytes.
#   distinct = 1   at N runs, the order did not move. That is weak evidence and
#                  the script says so: `Map.of` with a fixed key set can repeat
#                  an order by chance, so raise N before concluding anything.
#
# Measured 2026-09-05 on sayer-desktop, JDK 25.0.4, N=5: five distinct orders
# for the `toUnmodifiableMap` copy and four for `RECIPE_PROPERTY_SETS` itself.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
N="${1:-5}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="${RUSTMC_VANILLA_DIR:-$ROOT/oracle-work/vanilla-$VER}"
UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
LIBS="$VDIR/.libs"
[ -s "$UNSIGNED" ] || { echo "no unsigned jar at $UNSIGNED — run ci/recipe-match-golden.sh once first" >&2; exit 2; }
[ -d "$LIBS" ] || { echo "no unpacked libraries at $LIBS — run ci/recipe-match-golden.sh once first" >&2; exit 2; }

CP="$UNSIGNED:$(find "$LIBS" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/src/net/minecraft/world/item/crafting"
cat > "$WORK/src/net/minecraft/world/item/crafting/PropertySetOrderProbe.java" <<'JAVA'
package net.minecraft.world.item.crafting;

import java.lang.reflect.Field;
import java.util.Map;
import java.util.stream.Collectors;

/** Prints the iteration order of the two maps `update_recipes` is written from. */
public final class PropertySetOrderProbe {
    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        Field f = RecipeManager.class.getDeclaredField("RECIPE_PROPERTY_SETS");
        f.setAccessible(true);
        Map<?, ?> raw = (Map<?, ?>) f.get(null);
        // What `finalizeRecipeLoading` actually hands the packet.
        Map<Object, Object> collected = raw.entrySet().stream()
            .collect(Collectors.toUnmodifiableMap(Map.Entry::getKey, Map.Entry::getKey));
        net.minecraft.server.Bootstrap.STDOUT.println("RAW " + order(raw));
        net.minecraft.server.Bootstrap.STDOUT.println("COLLECTED " + order(collected));
    }

    private static String order(Map<?, ?> m) {
        StringBuilder sb = new StringBuilder();
        for (Object k : m.keySet()) {
            String s = k.toString();
            sb.append(s.substring(s.lastIndexOf(':') + 1).replace("]", "")).append(',');
        }
        return sb.toString();
    }

    private PropertySetOrderProbe() {
    }
}
JAVA
javac -nowarn -cp "$CP" -d "$WORK" "$WORK/src/net/minecraft/world/item/crafting/PropertySetOrderProbe.java" 2>&1 | grep -v '^Note' || true
[ -s "$WORK/net/minecraft/world/item/crafting/PropertySetOrderProbe.class" ] || { echo "the probe did not compile" >&2; exit 1; }

: > "$WORK/raw"; : > "$WORK/collected"
for _ in $(seq 1 "$N"); do
    out="$(java -cp "$CP:$WORK" net.minecraft.world.item.crafting.PropertySetOrderProbe 2>/dev/null)"
    echo "$out" | grep -a '^RAW ' >> "$WORK/raw"
    echo "$out" | grep -a '^COLLECTED ' >> "$WORK/collected"
done
[ "$(wc -l < "$WORK/collected")" -eq "$N" ] || { echo "the probe produced $(wc -l < "$WORK/collected") lines in $N runs — it did not run" >&2; exit 1; }

RD="$(sort -u "$WORK/raw" | wc -l)"
CD="$(sort -u "$WORK/collected" | wc -l)"
echo "recipe-packet-order: $N JVM runs -> $RD distinct RECIPE_PROPERTY_SETS orders, $CD distinct toUnmodifiableMap orders"
if [ "$CD" -gt 1 ]; then
    echo "recipe-packet-order: the update_recipes map order is a property of the JVM run."
    echo "recipe-packet-order: gate the CONTENT (property_sets in data/vanilla/recipe_match.json), not the bytes."
else
    echo "recipe-packet-order: WEAK — $N runs all agreed. A fixed key set can repeat an order by"
    echo "recipe-packet-order: chance; raise N before concluding the wire order is stable."
fi
