// FuelValuesGolden — `FuelValues.vanillaBurnTimes`, item by item.
//
// WHAT READS THIS
//
//   AbstractFurnaceBlockEntity.serverTick, offsets 207-217
//       int burnDuration = getBurnDuration(level.fuelValues(), fuelStack);
//       litTimeRemaining = litTotalTime = burnDuration;
//   AbstractFurnaceBlockEntity.canPlaceItem, offsets 24-35
//       level.fuelValues().isFuel(stack)      // which slot 1 will accept
//   FurnaceFuelSlot.mayPlace, and the hopper through `canPlaceItemInContainer`.
//
// So one map answers three questions and the arithmetic on it is nil:
// `burnDuration(stack)` is `values.getInt(stack.getItem())` after an `isEmpty`
// short-circuit, and `isFuel` is the same map's `containsKey`. There is no
// rule to transcribe — coal is 1600 and a wooden slab is 150 and nothing about
// either name says so — so this is a table or it is a guess.
//
// WHY THE MAP AND NOT A HAND LIST. `FuelValues.Builder` is fed from item
// **tags** (`ItemTags.LOGS`, `PLANKS`, `WOODEN_SLABS`, …) as well as from
// individual items, and a tag's membership is datapack-resolved rather than
// written in the builder. Transcribing the builder by hand would mean
// expanding those tags by hand, which is where a hand table goes wrong
// quietly and in exactly the direction that is hard to see: an item missing
// from the map is not a wrong burn time, it is a furnace that refuses fuel.
//
// A ZERO IS NOT AN ABSENCE, AND THIS DUMPER KEEPS THEM APART.
// `Object2IntSortedMap.getInt` returns the map's default (0) for an absent
// key, so `burnDuration` answers 0 for a stone and 0 for an item explicitly
// mapped to 0 — and `isFuel` distinguishes them. Only the items the map
// actually contains are emitted, taken from `fuelItems()` rather than by
// probing every item and keeping the non-zero answers, so a future fuel of
// duration 0 stays in the table where a probe would drop it.
//
// The FeatureFlagSet is `FeatureFlags.DEFAULT_FLAGS`, which is what a
// vanilla server without experimental packs runs; a wider set would put
// items in this table that no reachable server has.

package net.minecraft.world.level.levelgen;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

import com.google.gson.JsonElement;
import com.google.gson.JsonParser;

import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.Registry;
import net.minecraft.core.registries.Registries;
import net.minecraft.core.RegistryAccess;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.tags.TagKey;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.block.entity.FuelValues;

public final class FuelValuesGolden {
    private FuelValuesGolden() {}

    private static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // WITHOUT THIS THE DUMPER DOES NOT PRODUCE A WRONG TABLE, IT THROWS —
        // `FuelValues.Builder.add(TagKey, int)` iterates the tag's HolderSet
        // eagerly and an unbound one answers "can't be dereferenced during
        // construction". That is the good failure: a bound-but-empty tag
        // would have given a table missing every plank and log, silently.
        bindTags();
        // NOT `VanillaRegistries.createLookup()`. That builds a *fresh*
        // registry set whose item lookup is still under construction, so the
        // `HolderSet` it hands back for `ItemTags.LOGS` throws
        // "can't be dereferenced during construction" the moment
        // `FuelValues.Builder.add(TagKey, int)` iterates it — before any
        // datapack tag is consulted, and regardless of what has been bound.
        // The built-in registry-of-registries is the provider whose ITEM
        // lookup is the very registry `bindTags` above wrote into.
        HolderLookup.Provider lookup = RegistryAccess.fromRegistryOfRegistries(
            BuiltInRegistries.REGISTRY);
        FuelValues fuels = FuelValues.vanillaBurnTimes(lookup, FeatureFlags.DEFAULT_FLAGS);

        // THE MAP ITSELF, BY REFLECTION, AND NOT A PROBE OVER `burnDuration`.
        //
        // `new ItemStack(item)` cannot be constructed in a bootstrapped-only
        // JVM in 26.2 — `Holder$Reference.components` throws "Components not
        // bound yet", checked directly with a three-line probe rather than
        // inferred from this dumper's own stack trace — so neither
        // `burnDuration(stack)` nor `isFuel(stack)` can be called on anything
        // but `ItemStack.EMPTY`. Reading the private `values` map is not a
        // workaround for that: it is the more direct answer. Both public
        // methods are one map operation with no arithmetic — `burnDuration`
        // is `getInt(stack.getItem())` behind an `isEmpty` guard and `isFuel`
        // is `containsKey(stack.getItem())` with **no** guard at all — so the
        // map is the whole of what they know.
        @SuppressWarnings("unchecked")
        it.unimi.dsi.fastutil.objects.Object2IntSortedMap<Item> values;
        java.lang.reflect.Field f = FuelValues.class.getDeclaredField("values");
        f.setAccessible(true);
        values = (it.unimi.dsi.fastutil.objects.Object2IntSortedMap<Item>) f.get(fuels);

        // Iterated over `fuelItems()` rather than over the map's own entry
        // set, so that the emitted order is the order the public accessor
        // reports; the two are the same object and this is the one that a
        // consumer could reproduce.
        List<String> rows = new ArrayList<>();
        int zeroes = 0;
        int missing = 0;
        for (Item item : fuels.fuelItems()) {
            int d = values.getInt(item);
            if (d == 0) {
                zeroes++;
            }
            if (!values.containsKey(item)) {
                missing++;
            }
            rows.add("  {"
                + q("item") + ": " + q(BuiltInRegistries.ITEM.getKey(item).toString()) + ", "
                + q("burnDuration") + ": " + d
                + "}");
        }

        // The controls, and the two that can still be taken through the
        // public API because `ItemStack.EMPTY` needs no components.
        // `burnDuration(EMPTY)` is 0 from its own `isEmpty` branch at offsets
        // 0-8; `isFuel(EMPTY)` is 0 from `containsKey(Items.AIR)` and **not**
        // from an emptiness test, because `isFuel` has none — a reader who
        // assumed it did would be right by accident here and wrong the day
        // air entered the map.
        int emptyDuration = fuels.burnDuration(ItemStack.EMPTY);
        boolean emptyIsFuel = fuels.isFuel(ItemStack.EMPTY);
        Item stone = BuiltInRegistries.ITEM
            .getValue(Identifier.withDefaultNamespace("stone"));
        int stoneDuration = values.getInt(stone);
        boolean stoneIsFuel = values.containsKey(stone);

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append(q("fuels")).append(": ").append(rows.size()).append(",\n");
        out.append(q("zeroDurationFuels")).append(": ").append(zeroes).append(",\n");
        out.append(q("fuelItemsNotInMap")).append(": ").append(missing).append(",\n");
        out.append(q("emptyDuration")).append(": ").append(emptyDuration).append(",\n");
        out.append(q("emptyIsFuel")).append(": ").append(emptyIsFuel).append(",\n");
        out.append(q("stoneDuration")).append(": ").append(stoneDuration).append(",\n");
        out.append(q("stoneIsFuel")).append(": ").append(stoneIsFuel).append(",\n");
        out.append(q("table")).append(": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n]\n}\n");
        System.out.print(out);
    }

    /// Binds the jar's own item tags into `BuiltInRegistries.ITEM`.
    ///
    /// The same mechanism `LootGolden.bindTags` uses and for the same reason:
    /// `prepareTagReload(...).apply()` rather than `bindTags`, because the
    /// built-in registries are frozen once `Bootstrap.bootStrap()` returns and
    /// a reload is the path that works on a frozen registry. Only the item
    /// registry is needed here — `FuelValues.Builder` takes `TagKey<Item>`
    /// and nothing else.
    private static void bindTags() throws Exception {
        String dir = System.getProperty("rustmc.tags.dir");
        if (dir == null) {
            throw new IllegalStateException("-Drustmc.tags.dir is required");
        }
        java.nio.file.Path root = java.nio.file.Path.of(dir, "item");
        if (!java.nio.file.Files.isDirectory(root)) {
            throw new IllegalStateException("no item tags under " + root);
        }
        Map<String, JsonElement> raw = new TreeMap<>();
        try (var walk = java.nio.file.Files.walk(root)) {
            for (java.nio.file.Path p : walk.filter(x -> x.toString().endsWith(".json")).toList()) {
                String rel = root.relativize(p).toString();
                rel = rel.substring(0, rel.length() - ".json".length())
                    .replace(java.io.File.separatorChar, '/');
                try (var r = java.nio.file.Files.newBufferedReader(p, StandardCharsets.UTF_8)) {
                    raw.put("minecraft:" + rel, JsonParser.parseReader(r));
                }
            }
        }
        Registry<net.minecraft.world.item.Item> registry = BuiltInRegistries.ITEM;
        Map<TagKey<net.minecraft.world.item.Item>,
            List<net.minecraft.core.Holder<net.minecraft.world.item.Item>>> bound =
                new java.util.HashMap<>();
        for (String name : raw.keySet()) {
            List<net.minecraft.core.Holder<net.minecraft.world.item.Item>> holders =
                new ArrayList<>();
            for (String id : flatten(raw, name, new java.util.HashSet<>())) {
                Identifier ident = Identifier.tryParse(id);
                if (ident == null) {
                    continue;
                }
                registry.get(ResourceKey.create(Registries.ITEM, ident)).ifPresent(holders::add);
            }
            bound.put(TagKey.create(Registries.ITEM, Identifier.parse(name)), holders);
        }
        registry.prepareTagReload(new TagLoader.LoadResult<>(Registries.ITEM, bound)).apply();
    }

    /// Flattens one tag, following `#other_tag` entries transitively. `seen`
    /// breaks the cycle a malformed pack could contain, exactly as vanilla's
    /// own loader does.
    private static List<String> flatten(Map<String, JsonElement> raw, String name,
                                        java.util.Set<String> seen) {
        List<String> out = new ArrayList<>();
        if (!seen.add(name)) {
            return out;
        }
        JsonElement e = raw.get(name);
        if (e == null || !e.isJsonObject()) {
            return out;
        }
        var values = e.getAsJsonObject().getAsJsonArray("values");
        if (values == null) {
            return out;
        }
        for (JsonElement v : values) {
            String id;
            if (v.isJsonObject()) {
                var idEl = v.getAsJsonObject().get("id");
                id = idEl == null ? null : idEl.getAsString();
            } else {
                id = v.getAsString();
            }
            if (id == null || id.isEmpty()) {
                continue;
            }
            if (id.startsWith("#")) {
                String t = id.substring(1);
                out.addAll(flatten(raw, t.contains(":") ? t : "minecraft:" + t, seen));
            } else {
                out.add(id.contains(":") ? id : "minecraft:" + id);
            }
        }
        return out;
    }
}
