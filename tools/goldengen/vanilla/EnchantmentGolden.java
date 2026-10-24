// EnchantmentGolden — the 26.2 enchantment registry, as data.
//
// # What an enchantment IS in 26.2, and why this dumper exists
//
// It is not a Java subclass any more. `net/minecraft/world/item/enchantment/`
// holds `Enchantment` (one final class), `Enchantment$EnchantmentDefinition`
// and `Enchantment$Cost`, and every one of the 43 members is a **datapack
// registry entry**: a JSON document with `weight`, `max_level`, `min_cost`,
// `max_cost`, `anvil_cost`, `slots`, `supported_items`, an optional
// `primary_items`, an optional `exclusive_set` and an `effects` map. The jar
// ships all 43 under `data/minecraft/enchantment/`, and `Enchantments.class`
// is a bootstrap that builds the same 43 in code.
//
// So there is nothing to transcribe and everything to extract, and the extract
// has a cross-check nobody had to invent: vanilla's own Java bootstrap
// (`VanillaRegistries.createLookup()`) against vanilla's own shipped datapack
// (the jar entries). This dumper reads both and refuses if any of the 43
// disagree.
//
// # The definitions are copied verbatim and cross-checked, never re-typed
//
// Each entry lands in the table as the jar's own bytes for it, parsed and
// re-serialised and otherwise untouched. Re-typing `min_cost` as two ints
// would be a second reading of a thing vanilla already writes down, and a
// second reading is a second place to be wrong.
//
// The cross-check is the same 43 documents as Mojang's data generator writes
// them (`generated-data/data/minecraft/enchantment/`) against the 43 the jar
// bundles (`data/minecraft/enchantment/`). Different code paths at different
// times over the same bootstrap, so a mismatch is a real finding and the
// count of files read is asserted beside the count of mismatches — a reader
// that found no files reports no mismatches and looks perfect.
//
// **`Enchantment.DIRECT_CODEC.encodeStart` was tried first and abandoned, and
// the reason is worth keeping.** A `HolderSet.Named` cannot be encoded until
// the tag it names is bound, and `exclusive_set` names tags in the
// **ENCHANTMENT** registry — a datapack registry, whose `MappedRegistry` a
// harness holding only a `HolderLookup.Provider` cannot reach to call
// `prepareTagReload` on. Binding item, entity_type and block tags gets three
// quarters of the way and then stops. The jar's own bytes need none of it.
//
// # What this dumper adds that neither source has on its own
//
//  * **registry order.** `EnchantmentMenu.slotsChanged` writes
//    `Registry.asHolderIdMap().getId(holder)` into `enchantClue`, so the
//    numeric id of every enchantment is observable on the wire.
//
//    **It is NOT the order `VanillaRegistries.createLookup()` iterates, and
//    that was this dumper's first answer.** That provider hands back
//    `thorns, unbreaking, swift_sneak, fire_protection, …` — the bootstrap's
//    own registration order — while a live 26.2 server's `registry_data`
//    packet for `minecraft:enchantment` carries
//    `aqua_affinity, bane_of_arthropods, binding_curse, …`: strictly sorted by
//    identifier, because a datapack registry is populated by
//    `RegistryDataLoader` walking the resource manager, whose listing is a
//    sorted map. Checked against `data/vanilla/registries.bin`, which is a
//    capture of exactly that packet from a real server; all 43 agree in order.
//    So this table sorts, and `crates/rustmc-server/tests/enchantment_registry_order.rs`
//    re-derives the wire order from that capture and refuses a table that
//    disagrees. A plausible order here would have given every `enchantClue`
//    on the wire the wrong id, and nothing else in the corpus would have
//    noticed.
//  * **resolved tags.** `supported_items`, `primary_items` and
//    `exclusive_set` are HolderSets, usually written as one `#tag`. The tag
//    contents are resolved here, in `TagLoader`'s order: that class builds
//    into a `LinkedHashSet` and finishes with `List.copyOf`, so a tag's order
//    is first-occurrence order over its own `values` array with nested tags
//    spliced in place. That order is load-bearing — `WeightedRandom`
//    subtracts weights walking the list — so it is extracted rather than
//    assumed.
//  * **every enchantment tag**, resolved the same way, including
//    `in_enchanting_table` — the one `EnchantmentMenu.getEnchantmentList`
//    streams. The others are what the loot functions and the trade providers
//    select over (`#on_random_loot`, `#tradeable`, `#on_mob_spawn_equipment`),
//    and `#treasure`/`#non_treasure` are the split that decides whether an
//    enchanting table can offer an entry at all.
//  * **the three item-side numbers the enchanting and anvil maths read**:
//    `DataComponents.ENCHANTABLE` (the enchantability that scales
//    `selectEnchantment`'s level roll), `MAX_DAMAGE` (the anvil's repair
//    arithmetic) and `REPAIRABLE` (which material repairs which tool).
//
// Run through `ci/enchantments-golden.sh`. One JVM, one output file.
package net.minecraft.world.level.levelgen;

import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.TreeSet;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.item.Item;

public final class EnchantmentGolden {
    private static final Gson GSON = new GsonBuilder().create();

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        HolderLookup.Provider registries = VanillaRegistries.createLookup();

        // Item data components have to be bound before `Item.components()`
        // answers anything — the same step `LootGolden` needs before it can
        // build an `ItemStack`. Without it every enchantability below reads
        // as absent and the table is a plausible file describing an
        // unenchantable game.
        for (var pending : BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(registries)) {
            pending.apply();
        }

        // Item tags have to be bound before `Repairable.items()` can be walked:
        // an unbound `HolderSet.Named` throws "can't be dereferenced during
        // construction" rather than reading empty, which is the lucky failure
        // — the same set read through `ItemStack.is(HolderSet)` answers a
        // silent `false` for every item, which is `LootGolden`'s
        // `#cluster_max_harvestables` defect with a different tag.
        //
        // `prepareTagReload(...).apply()`, not `bindTags`: the built-in
        // registries are frozen once `Bootstrap.bootStrap()` returns and only
        // the reload path works on a frozen registry.
        bindOne(req("rustmc.tags.dir") + "/item", BuiltInRegistries.ITEM, Registries.ITEM);

        var reg = registries.lookupOrThrow(Registries.ENCHANTMENT);

        // --- registry order, from vanilla's own bootstrap -------------------
        List<String> order = new ArrayList<>();
        reg.listElements().forEach(h -> order.add(h.key().identifier().toString()));
        // Sorted, not as iterated — see the header. `RegistryDataLoader`'s
        // resource listing is a sorted map, so this is the order a live
        // server's registry takes and the order `enchantClue` indexes into.
        java.util.Collections.sort(order);

        // --- the shipped-datapack side, straight out of the jar ------------
        Map<String, JsonElement> shipped = new TreeMap<>();
        try (var zip = new java.util.zip.ZipFile(req("rustmc.jar"))) {
            var entries = zip.entries();
            while (entries.hasMoreElements()) {
                var e = entries.nextElement();
                String n = e.getName();
                if (!n.startsWith("data/minecraft/enchantment/") || !n.endsWith(".json")) continue;
                String id = "minecraft:" + n.substring("data/minecraft/enchantment/".length(),
                        n.length() - ".json".length());
                try (var r = new java.io.InputStreamReader(zip.getInputStream(e), StandardCharsets.UTF_8)) {
                    shipped.put(id, JsonParser.parseReader(r));
                }
            }
        }

        // --- and the data generator's side ---------------------------------
        String genDir = req("rustmc.generated.dir");
        Map<String, JsonElement> generated = new TreeMap<>();
        java.nio.file.Path gRoot = java.nio.file.Path.of(genDir);
        if (java.nio.file.Files.isDirectory(gRoot)) {
            try (var walk = java.nio.file.Files.walk(gRoot)) {
                for (java.nio.file.Path p : walk.filter(x -> x.toString().endsWith(".json")).toList()) {
                    String rel = gRoot.relativize(p).toString();
                    rel = rel.substring(0, rel.length() - ".json".length())
                            .replace(java.io.File.separatorChar, '/');
                    try (var r = java.nio.file.Files.newBufferedReader(p, StandardCharsets.UTF_8)) {
                        generated.put("minecraft:" + rel, JsonParser.parseReader(r));
                    }
                }
            }
        }

        // --- the cross-check -----------------------------------------------
        //
        // Objects are canonicalised (recursively sorted keys) before comparing,
        // because Gson preserves insertion order and the two writers need not
        // agree on field order. Arrays are compared AS THEY ARE: `slots` and
        // every HolderSet list is ordered, and a reordering is a difference.
        //
        // Membership is checked against the code registry as well, so a
        // bootstrap that gained or lost an entry is caught even though the two
        // JSON sides would still agree with each other.
        List<String> mismatches = new ArrayList<>();
        for (String id : new TreeSet<>(union(union(shipped.keySet(), generated.keySet()),
                                             new java.util.LinkedHashSet<>(order)))) {
            JsonElement a = shipped.get(id), b = generated.get(id);
            if (!order.contains(id)) { mismatches.add(id + ": on disk, not in the code registry"); continue; }
            if (a == null) { mismatches.add(id + ": in the code registry, not bundled in the jar"); continue; }
            if (b == null) { mismatches.add(id + ": bundled in the jar, not written by the data generator"); continue; }
            if (!canon(a).equals(canon(b))) mismatches.add(id + ": jar bundle and generated data differ");
        }

        Map<String, JsonElement> encoded = new java.util.LinkedHashMap<>();
        for (String id : order) encoded.put(id, shipped.get(id));

        // --- tags -----------------------------------------------------------
        String tagsDir = req("rustmc.tags.dir");
        Map<String, JsonElement> enchTags = readTagDir(tagsDir + "/enchantment");
        Map<String, JsonElement> itemTags = readTagDir(tagsDir + "/item");

        List<String> inTable = flatten(enchTags, "minecraft:in_enchanting_table");
        Map<String, List<String>> resolvedTags = new TreeMap<>();
        for (String name : enchTags.keySet()) {
            resolvedTags.put(name, flatten(enchTags, name));
        }

        // --- item-side numbers ------------------------------------------------
        Map<String, Integer> enchantability = new TreeMap<>();
        Map<String, Integer> maxDamage = new TreeMap<>();
        Map<String, JsonElement> repairable = new TreeMap<>();
        for (Identifier id : new TreeSet<>(BuiltInRegistries.ITEM.keySet())) {
            Item item = BuiltInRegistries.ITEM.getValue(id);
            var comps = item.components();
            var ench = comps.get(DataComponents.ENCHANTABLE);
            if (ench != null) enchantability.put(id.toString(), ench.value());
            var md = comps.get(DataComponents.MAX_DAMAGE);
            if (md != null) maxDamage.put(id.toString(), md);
            var rep = comps.get(DataComponents.REPAIRABLE);
            if (rep != null) repairable.put(id.toString(), repairMaterials(rep));
        }

        // --- write -------------------------------------------------------------
        // NOT System.out: `Bootstrap.bootStrap()` has redirected it through
        // log4j, so a JSON document written there comes back line-prefixed and
        // unparseable. Same reason `LootGolden` takes a -D path.
        PrintStream out = new PrintStream(new java.io.BufferedOutputStream(
                java.nio.file.Files.newOutputStream(java.nio.file.Path.of(req("rustmc.enchantments.out"))), 1 << 20),
                false, StandardCharsets.UTF_8);
        JsonObject root = new JsonObject();
        root.addProperty("version", SharedConstants.getCurrentVersion().name());
        root.addProperty("count", order.size());
        root.addProperty("shipped_files", shipped.size());
        root.addProperty("generated_files", generated.size());
        root.addProperty("crosscheck_mismatches", mismatches.size());
        root.add("order", GSON.toJsonTree(order));
        root.add("in_enchanting_table", GSON.toJsonTree(inTable));
        root.add("tags", GSON.toJsonTree(resolvedTags));

        JsonObject defs = new JsonObject();
        for (String id : order) defs.add(id, encoded.get(id));
        root.add("enchantments", defs);

        JsonObject resolved = new JsonObject();
        for (String id : order) {
            JsonObject o = encoded.get(id).getAsJsonObject();
            JsonObject r = new JsonObject();
            r.add("supported_items", GSON.toJsonTree(resolveItems(itemTags, o.get("supported_items"))));
            r.add("primary_items", o.has("primary_items")
                    ? GSON.toJsonTree(resolveItems(itemTags, o.get("primary_items")))
                    : com.google.gson.JsonNull.INSTANCE);
            r.add("exclusive_set", GSON.toJsonTree(o.has("exclusive_set")
                    ? resolveEnch(enchTags, o.get("exclusive_set"))
                    : List.<String>of()));
            resolved.add(id, r);
        }
        root.add("resolved", resolved);

        root.add("enchantability", GSON.toJsonTree(enchantability));
        root.add("max_damage", GSON.toJsonTree(maxDamage));
        JsonObject rep = new JsonObject();
        for (var e : repairable.entrySet()) rep.add(e.getKey(), e.getValue());
        root.add("repairable", rep);

        out.println(GSON.toJson(root));
        out.flush();
        out.close();

        Bootstrap.STDOUT.println("enchantments: " + order.size() + " in the code registry, "
                + shipped.size() + " bundled in the jar, " + generated.size()
                + " written by the data generator, " + mismatches.size() + " mismatches");
        Bootstrap.STDOUT.println("enchantments: " + resolvedTags.size() + " tags, in_enchanting_table = "
                + inTable.size() + ", enchantability on " + enchantability.size() + " items");
        for (String m : mismatches) Bootstrap.STDOUT.println("  !! " + m);
        if (!mismatches.isEmpty()) System.exit(1);
    }

    /// `Repairable.items()` is a `HolderSet<Item>`; item tags are bound above,
    /// so it dereferences. Emitted as the resolved item list rather than as
    /// the tag name, because `ItemStack.isValidRepairItem` is a membership
    /// test and a name would make the Rust side resolve the tag a second time.
    private static JsonElement repairMaterials(net.minecraft.world.item.enchantment.Repairable rep) {
        List<String> ids = new ArrayList<>();
        rep.items().forEach(h -> ids.add(h.unwrapKey().orElseThrow().identifier().toString()));
        return GSON.toJsonTree(ids);
    }

    // ---- tag resolution ---------------------------------------------------
    //
    // `TagLoader.build` collects into a `LinkedHashSet` and finishes with
    // `List.copyOf`, so: first-occurrence order, nested tags spliced where
    // they appear, duplicates dropped. Reproduced exactly, because the order
    // is what `WeightedRandom.getWeightedItem` walks.

    private static <T> void bindOne(String dir, net.minecraft.core.Registry<T> registry,
                                    net.minecraft.resources.ResourceKey<? extends net.minecraft.core.Registry<T>> key)
            throws Exception {
        Map<String, JsonElement> raw = readTagDir(dir);
        if (raw.isEmpty()) return;
        Map<net.minecraft.tags.TagKey<T>, List<net.minecraft.core.Holder<T>>> bound =
                new java.util.HashMap<>();
        for (String name : raw.keySet()) {
            List<net.minecraft.core.Holder<T>> holders = new ArrayList<>();
            for (String id : flatten(raw, name)) {
                Identifier ident = Identifier.tryParse(id);
                if (ident == null) continue;
                registry.get(net.minecraft.resources.ResourceKey.create(key, ident)).ifPresent(holders::add);
            }
            bound.put(net.minecraft.tags.TagKey.create(key, Identifier.parse(name)), holders);
        }
        registry.prepareTagReload(new net.minecraft.tags.TagLoader.LoadResult<>(key, bound)).apply();
    }

    private static Map<String, JsonElement> readTagDir(String dir) throws Exception {
        Map<String, JsonElement> raw = new TreeMap<>();
        java.nio.file.Path root = java.nio.file.Path.of(dir);
        if (!java.nio.file.Files.isDirectory(root)) return raw;
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
        return raw;
    }

    private static List<String> flatten(Map<String, JsonElement> raw, String name) {
        LinkedHashSet<String> out = new LinkedHashSet<>();
        flattenInto(raw, name, new java.util.HashSet<>(), out);
        return List.copyOf(out);
    }

    private static void flattenInto(Map<String, JsonElement> raw, String name,
                                    java.util.Set<String> seen, LinkedHashSet<String> out) {
        if (!seen.add(name)) return;
        JsonElement e = raw.get(name);
        if (e == null || !e.isJsonObject()) return;
        var values = e.getAsJsonObject().getAsJsonArray("values");
        if (values == null) return;
        for (JsonElement v : values) {
            String id = v.isJsonObject()
                    ? (v.getAsJsonObject().get("id") == null ? null : v.getAsJsonObject().get("id").getAsString())
                    : v.getAsString();
            if (id == null) continue;
            if (id.startsWith("#")) {
                String sub = id.substring(1);
                if (!sub.contains(":")) sub = "minecraft:" + sub;
                flattenInto(raw, sub, seen, out);
            } else {
                out.add(id.contains(":") ? id : "minecraft:" + id);
            }
        }
    }

    /// A `HolderSet` in JSON is either `"#tag"`, a single id, or an array of
    /// ids. All three shapes appear across the 43, so all three are handled;
    /// a shape that is none of them is an error rather than an empty list,
    /// because an empty `supported_items` is an enchantment that applies to
    /// nothing and looks exactly like a correct extract.
    private static List<String> resolveSet(Map<String, JsonElement> tags, JsonElement e) {
        LinkedHashSet<String> out = new LinkedHashSet<>();
        if (e.isJsonArray()) {
            for (JsonElement v : e.getAsJsonArray()) out.addAll(resolveSet(tags, v));
        } else if (e.isJsonPrimitive()) {
            String s = e.getAsString();
            if (s.startsWith("#")) {
                String sub = s.substring(1);
                if (!sub.contains(":")) sub = "minecraft:" + sub;
                out.addAll(flatten(tags, sub));
            } else {
                out.add(s.contains(":") ? s : "minecraft:" + s);
            }
        } else {
            throw new IllegalStateException("unhandled HolderSet shape: " + e);
        }
        return List.copyOf(out);
    }

    private static List<String> resolveItems(Map<String, JsonElement> tags, JsonElement e) {
        return resolveSet(tags, e);
    }

    private static List<String> resolveEnch(Map<String, JsonElement> tags, JsonElement e) {
        return resolveSet(tags, e);
    }

    // ---- helpers ----------------------------------------------------------

    private static <T> java.util.Set<T> union(java.util.Set<T> a, java.util.Set<T> b) {
        var s = new java.util.LinkedHashSet<T>(a);
        s.addAll(b);
        return s;
    }

    /// Recursively key-sorted, so a field-order difference between the codec
    /// and the datapack writer is not read as a value difference. Array order
    /// is preserved deliberately: `slots` and every HolderSet list are ordered
    /// and a reordering IS a difference.
    private static JsonElement canon(JsonElement e) {
        if (e.isJsonObject()) {
            JsonObject o = new JsonObject();
            for (String k : new TreeSet<>(e.getAsJsonObject().keySet())) {
                o.add(k, canon(e.getAsJsonObject().get(k)));
            }
            return o;
        }
        if (e.isJsonArray()) {
            var a = new com.google.gson.JsonArray();
            for (JsonElement v : e.getAsJsonArray()) a.add(canon(v));
            return a;
        }
        return e;
    }

    private static String req(String key) {
        String v = System.getProperty(key);
        if (v == null) throw new IllegalStateException("-D" + key + " is required");
        return v;
    }

    private EnchantmentGolden() {}
}
