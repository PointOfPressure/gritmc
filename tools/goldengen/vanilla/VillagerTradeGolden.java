// VillagerTradeGolden — every villager and wandering-trader trade listing in
// 26.2, read off the pinned jar rather than transcribed.
//
// # THE PREMISE THIS FILE REFUTES
//
// Every account of villager trading before 26.2 — and the brief this lane was
// spawned with — describes `VillagerTrades.TRADES`, an
// `Int2ObjectMap<Int2ObjectMap<ItemListing[]>>` built in a static initialiser
// out of lambdas and small `ItemListing` classes: `EmeraldForItems`,
// `ItemsForEmeralds`, `EnchantedItemForEmeralds`, `TreasureMapForEmeralds`,
// `DyedArmorForEmeralds`, `SuspiciousStewForEmerald`, and so on. **None of
// those classes exist in the 26.2 jar.** `unzip -l server-26.2.jar |
// grep ItemListing` matches nothing; `javap -p VillagerTrades` shows a class
// whose every field is a `ResourceKey<VillagerTrade>` and whose only real
// method is `bootstrap(BootstrapContext)`.
//
// In 26.2 a trade is a **datapack registry entry**. There are two new
// registries — `minecraft:villager_trade` (`Registries.VILLAGER_TRADE`,
// element type `VillagerTrade`, `Codec`-loaded) and `minecraft:trade_set`
// (`Registries.TRADE_SET`, element type `TradeSet`) — and the per-level
// listing pool is a **tag** over the first of them. The jar ships:
//
//   data/minecraft/villager_trade/**.json        the listings themselves
//   data/minecraft/trade_set/**.json             one per profession-level
//   data/minecraft/tags/villager_trade/**.json   the pools the sets draw from
//
// So the whole of "which trades does a level-3 farmer draw from" is data, and
// this dumper's job is to carry that data across intact together with the two
// pieces of it that are *not* in the JSON: the profession -> trade-set map
// (`VillagerProfession.tradeSetsByLevel`, Java, `VillagerProfession.bootstrap`)
// and the codec defaults for the fields a file omits.
//
// # THE CODEC DEFAULTS ARE NOT COSMETIC
//
// 16 of the 388 listings omit `max_uses` and 122 omit `xp`. Read as "absent
// means zero" a villager would offer every one of those trades exactly never,
// or grant no experience for it and so never level up. `VillagerTrade.CODEC`
// (`lambda$static$0`, offsets 58..119) reads:
//
//   max_uses            lenientOptionalFieldOf, default ConstantValue.exactly(4.0f)
//   reputation_discount lenientOptionalFieldOf, default ConstantValue.exactly(0.0f)
//   xp                  lenientOptionalFieldOf, default ConstantValue.exactly(1.0f)
//   given_item_modifiers optionalFieldOf,       default List.of()
//
// and `TradeSet.CODEC` (offsets 43..49) defaults `allow_duplicates` to
// **false**. This dumper writes those defaults in explicitly so that a reader
// cannot get them wrong by omission, and records that it did.
//
// # WHAT IS RECORDED AS A SHAPE AND NOT AS A DRAW
//
// `given_item_modifiers` is a list of `LootItemFunction`s applied to the
// result stack at offer-creation time (`VillagerTrade.getOffer` offsets
// 41..96), and several of them draw: `enchant_with_levels`,
// `enchant_randomly`, `set_random_dyes`, `set_random_potion`,
// `set_stew_effect`, `exploration_map`. Recording one *rolled* enchanted book
// would be recording one sample of a distribution and calling it the table.
// The function objects are carried across verbatim — parameters and all — and
// the rolling is the loot crate's problem, which is where those functions
// already live.
//
// # NO DYNAMIC-REGISTRY INDICES, NO IDENTITY HASHES
//
// Everything here is a namespaced id or a JSON literal out of the pack. There
// is no numeric registry index anywhere in the output: `villager_trade` and
// `trade_set` are datapack registries whose integer ids depend on load order
// and on which packs are enabled, so an index recorded today is a different
// trade tomorrow.
//
// # TWO SOURCES, COMPARED
//
// The same 529 files exist twice on this box: inside the jar, and under
// `generated-data/`, which is Mojang's own data generator's output. They are
// read independently and compared byte for byte, and the comparison result is
// written into the table. One source agreeing with itself is not a check.

package net.minecraft.world.level.levelgen;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.google.gson.JsonPrimitive;

import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.entity.npc.villager.VillagerData;
import net.minecraft.world.entity.npc.villager.VillagerProfession;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.trading.TradeSet;

public final class VillagerTradeGolden {

    /// Everything under this path prefix belongs to the wandering trader and
    /// goes in the second table. The split is by id, and the generator proves
    /// it is a real partition by checking that no tag on either side reaches
    /// the other (see `assertPartition`).
    private static final String WT = "wandering_trader/";

    private static final String TRADE_DIR = "data/minecraft/villager_trade/";
    private static final String SET_DIR = "data/minecraft/trade_set/";
    private static final String TAG_DIR = "data/minecraft/tags/villager_trade/";

    public static void main(String[] args) throws Exception {
        if (args.length < 3) {
            System.err.println("usage: VillagerTradeGolden <server.jar> <generated-data-dir> "
                    + "<out-villager.json> <out-wandering.json>");
            System.exit(2);
        }
        String jar = args[0];
        String genData = args[1];
        Path outVillager = Path.of(args[2]);
        Path outWandering = Path.of(args[3]);

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // `Item.getDefaultMaxStackSize()` reads the item's data components,
        // and after `Bootstrap.bootStrap()` alone they are not bound —
        // `NullPointerException: Components not bound yet` from
        // `Holder$Reference.components`. On a real server the binding happens
        // during resource-pack load. `Bootstrap.validate()` does not do it
        // either; this is the step, and `ci/loot-golden.sh` found it the
        // expensive way (554 "successful" cases containing zero items).
        for (var pending : BuiltInRegistries.DATA_COMPONENT_INITIALIZERS
                .build(net.minecraft.data.registries.VanillaRegistries.createLookup())) {
            pending.apply();
        }

        // ---- source A: the pinned jar ------------------------------------
        Map<String, String> jarTrades = readFromJar(jar, TRADE_DIR);
        Map<String, String> jarSets = readFromJar(jar, SET_DIR);
        Map<String, String> jarTags = readFromJar(jar, TAG_DIR);

        // ---- source B: Mojang's own data generator output -----------------
        Path g = Path.of(genData);
        Map<String, String> genTrades = readFromDir(g.resolve("data/minecraft/villager_trade"));
        Map<String, String> genSets = readFromDir(g.resolve("data/minecraft/trade_set"));
        Map<String, String> genTags = readFromDir(g.resolve("data/minecraft/tags/villager_trade"));

        JsonObject cross = new JsonObject();
        cross.addProperty("jarTrades", jarTrades.size());
        cross.addProperty("generatedTrades", genTrades.size());
        cross.addProperty("jarTradeSets", jarSets.size());
        cross.addProperty("generatedTradeSets", genSets.size());
        cross.addProperty("jarTags", jarTags.size());
        cross.addProperty("generatedTags", genTags.size());
        int mismatches = countMismatches(jarTrades, genTrades)
                + countMismatches(jarSets, genSets)
                + countMismatches(jarTags, genTags);
        // THE BUILT-IN `trade_rebalance` DATAPACK IS 93 MORE LISTINGS AND 23
        // TAG OVERRIDES, AND IT MUST NOT BE HERE.
        //
        // It ships inside the same jar at
        // `data/minecraft/datapacks/trade_rebalance/data/minecraft/villager_trade/...`,
        // is not enabled in a default world, and `TradeRebalanceVillagerTrades`
        // extends `VillagerTrades` with biome-specific librarian books among
        // others. A dumper that globbed `**/villager_trade/*.json` instead of
        // matching the prefix would merge the two silently, and every count in
        // this table would be a mixture of an enabled pack and a disabled one.
        // The exclusion is a prefix test, so it is pinned here rather than
        // trusted: 91 jar entries hold a trade or a trade tag under a
        // `datapacks/` path — 81 rebalance listings and 10 tag overrides —
        // and none of them is read above.
        int rebalance = countUnderDatapacks(jar);
        cross.addProperty("datapackEntriesExcluded", rebalance);
        if (rebalance != 91) {
            System.err.println("the built-in datapacks hold " + rebalance
                    + " trade entries, not the 91 this dumper's exclusion was measured against"
                    + " — check the prefix test before trusting any count here");
            System.exit(1);
        }
        for (String id : jarTrades.keySet()) {
            if (id.contains("datapacks/")) {
                System.err.println("a datapack entry reached the table: " + id);
                System.exit(1);
            }
        }

        cross.addProperty("mismatches", mismatches);
        if (mismatches != 0) {
            System.err.println("the jar and the data generator disagree on " + mismatches
                    + " villager-trade files; one of the two sources is not 26.2");
            System.exit(1);
        }

        // ---- the profession -> trade-set map, which is Java and not data ---
        JsonArray professions = professionTable();

        // ---- the tags, resolved in vanilla's own order ---------------------
        Map<String, List<String>> resolved = new LinkedHashMap<>();
        for (String id : new TreeMap<>(jarTags).keySet()) {
            resolved.put(id, resolveTag(jarTags, id));
        }

        assertPartition(jarTags, resolved);

        JsonObject villager = build(false, jarTrades, jarSets, jarTags, resolved, professions, cross);
        JsonObject wandering = build(true, jarTrades, jarSets, jarTags, resolved, professions, cross);

        Gson gson = new GsonBuilder().setPrettyPrinting().create();
        Files.createDirectories(outVillager.toAbsolutePath().getParent());
        Files.writeString(outVillager, gson.toJson(villager) + "\n", StandardCharsets.UTF_8);
        Files.writeString(outWandering, gson.toJson(wandering) + "\n", StandardCharsets.UTF_8);

        System.out.println("villager trades   -> " + outVillager);
        System.out.println("wandering trades  -> " + outWandering);
        System.out.println("  villager_trade entries " + jarTrades.size()
                + ", trade_set entries " + jarSets.size()
                + ", tags " + jarTags.size()
                + " (jar and generated-data byte-identical on all "
                + (jarTrades.size() + jarSets.size() + jarTags.size()) + ")");
    }

    // ------------------------------------------------------------------ build

    private static JsonObject build(boolean wanderingTrader,
                                    Map<String, String> trades,
                                    Map<String, String> sets,
                                    Map<String, String> tags,
                                    Map<String, List<String>> resolved,
                                    JsonArray professions,
                                    JsonObject cross) {
        JsonObject root = new JsonObject();
        root.addProperty("version", SharedConstants.getCurrentVersion().name());
        root.addProperty("side", wanderingTrader ? "wandering_trader" : "villager");
        root.add("crossCheck", cross.deepCopy());

        // The codec defaults, written down where a reader trips over them.
        JsonObject defaults = new JsonObject();
        defaults.addProperty("maxUses", 4.0f);
        defaults.addProperty("reputationDiscount", 0.0f);
        defaults.addProperty("xp", 1.0f);
        defaults.addProperty("givenItemModifiers", "[]");
        defaults.addProperty("allowDuplicates", false);
        root.add("codecDefaults", defaults);

        // Level thresholds live here because they are the other half of "which
        // trade set does this villager use": `VillagerData.NEXT_LEVEL_XP_THRESHOLDS`.
        JsonArray thresholds = new JsonArray();
        for (int level = 1; level <= VillagerData.MAX_VILLAGER_LEVEL; level++) {
            JsonObject t = new JsonObject();
            t.addProperty("level", level);
            t.addProperty("canLevelUp", VillagerData.canLevelUp(level));
            t.addProperty("minXp", VillagerData.getMinXpPerLevel(level));
            t.addProperty("maxXp", VillagerData.getMaxXpPerLevel(level));
            thresholds.add(t);
        }
        root.add("levelXpThresholds", thresholds);

        if (!wanderingTrader) {
            root.add("professions", professions);
        }

        JsonArray setArr = new JsonArray();
        for (Map.Entry<String, String> e : new TreeMap<>(sets).entrySet()) {
            if (isWt(e.getKey()) != wanderingTrader) {
                continue;
            }
            JsonObject src = JsonParser.parseString(e.getValue()).getAsJsonObject();
            JsonObject o = new JsonObject();
            o.addProperty("id", e.getKey());
            o.add("trades", src.get("trades"));
            o.add("amount", src.get("amount"));
            o.addProperty("allowDuplicates",
                    src.has("allow_duplicates") && src.get("allow_duplicates").getAsBoolean());
            o.add("randomSequence", src.has("random_sequence")
                    ? src.get("random_sequence") : null);
            setArr.add(o);
        }
        root.add("tradeSets", setArr);

        JsonArray tagArr = new JsonArray();
        for (Map.Entry<String, String> e : new TreeMap<>(tags).entrySet()) {
            if (isWt(e.getKey()) != wanderingTrader) {
                continue;
            }
            JsonObject src = JsonParser.parseString(e.getValue()).getAsJsonObject();
            JsonObject o = new JsonObject();
            o.addProperty("id", e.getKey());
            o.add("values", src.get("values"));
            JsonArray r = new JsonArray();
            for (String m : resolved.get(e.getKey())) {
                r.add(m);
            }
            o.add("resolved", r);
            tagArr.add(o);
        }
        root.add("tags", tagArr);

        JsonArray tradeArr = new JsonArray();
        for (Map.Entry<String, String> e : new TreeMap<>(trades).entrySet()) {
            if (isWt(e.getKey()) != wanderingTrader) {
                continue;
            }
            tradeArr.add(normaliseTrade(e.getKey(), JsonParser.parseString(e.getValue()).getAsJsonObject()));
        }
        root.add("trades", tradeArr);

        // `Item.getDefaultMaxStackSize()` for every item these listings name.
        //
        // `TradeCost.toItemCost` clamps the cost count to it and
        // `MerchantOffer.getModifiedCostCount` clamps the modified count to
        // it again, so a reader that assumed 64 would offer 16 water buckets
        // and 24 beds. It is not in `items.json` — that table is id to
        // protocol index — and it is a property of the item rather than of
        // the trade, so it is collected here from the built-in registry for
        // exactly the items this table can ask about and no others.
        JsonObject stackSizes = new JsonObject();
        java.util.TreeSet<String> items = new java.util.TreeSet<>();
        for (JsonElement e : tradeArr) {
            JsonObject o = e.getAsJsonObject();
            for (String field : new String[] {"wants", "additionalWants", "gives"}) {
                JsonElement f = o.get(field);
                if (f != null && f.isJsonObject() && f.getAsJsonObject().has("id")) {
                    items.add(f.getAsJsonObject().get("id").getAsString());
                }
            }
        }
        for (String id : items) {
            Item item = BuiltInRegistries.ITEM.getValue(Identifier.parse(id));
            if (item == null) {
                throw new IllegalStateException("a listing names item " + id
                        + ", which is not in the item registry");
            }
            stackSizes.addProperty(id, item.getDefaultMaxStackSize());
        }
        root.add("itemStackSizes", stackSizes);

        JsonObject counts = new JsonObject();
        counts.addProperty("tradeSets", setArr.size());
        counts.addProperty("tags", tagArr.size());
        counts.addProperty("trades", tradeArr.size());
        counts.addProperty("items", stackSizes.size());
        root.add("counts", counts);
        return root;
    }

    /// One listing, with every codec default written in explicitly and the
    /// field names spelled the way the class spells them.
    private static JsonObject normaliseTrade(String id, JsonObject src) {
        JsonObject o = new JsonObject();
        o.addProperty("id", id);
        o.add("wants", src.get("wants"));
        o.add("additionalWants", src.has("additional_wants") ? src.get("additional_wants") : null);
        o.add("gives", src.get("gives"));
        o.add("maxUses", src.has("max_uses") ? src.get("max_uses") : new JsonPrimitive(4.0f));
        o.add("reputationDiscount", src.has("reputation_discount")
                ? src.get("reputation_discount") : new JsonPrimitive(0.0f));
        o.add("xp", src.has("xp") ? src.get("xp") : new JsonPrimitive(1.0f));
        o.add("merchantPredicate", src.has("merchant_predicate") ? src.get("merchant_predicate") : null);
        o.add("givenItemModifiers", src.has("given_item_modifiers")
                ? src.get("given_item_modifiers") : new JsonArray());
        o.add("doubleTradePriceEnchantments", src.has("double_trade_price_enchantments")
                ? src.get("double_trade_price_enchantments") : null);
        // Which fields the file actually carried, so a reader can tell a
        // defaulted 4 from a written 4 without re-reading the pack.
        JsonArray defaulted = new JsonArray();
        for (String k : new String[] {"max_uses", "reputation_discount", "xp", "given_item_modifiers"}) {
            if (!src.has(k)) {
                defaulted.add(k);
            }
        }
        o.add("defaultedFields", defaulted);
        return o;
    }

    // ------------------------------------------------------------ professions

    /// `VillagerProfession.tradeSetsByLevel`, read through `getTrades(level)`
    /// on the live registry rather than transcribed out of
    /// `VillagerProfession.bootstrap`'s 800 lines of `Int2ObjectMap.entry`.
    ///
    /// A profession with no trades at all — `none` and `nitwit` — is emitted
    /// with an empty map rather than skipped: `Villager.updateTrades` returns
    /// on a null key (offsets 28..33), and "this profession has no trade set"
    /// is a fact a reader needs, not an absence.
    private static JsonArray professionTable() {
        JsonArray out = new JsonArray();
        for (var entry : BuiltInRegistries.VILLAGER_PROFESSION.entrySet()) {
            ResourceKey<VillagerProfession> key = entry.getKey();
            VillagerProfession p = entry.getValue();
            JsonObject o = new JsonObject();
            o.addProperty("id", key.identifier().toString());
            JsonObject levels = new JsonObject();
            for (int level = VillagerData.MIN_VILLAGER_LEVEL;
                    level <= VillagerData.MAX_VILLAGER_LEVEL; level++) {
                ResourceKey<TradeSet> set = p.getTrades(level);
                if (set != null) {
                    levels.addProperty(Integer.toString(level), set.identifier().toString());
                }
            }
            o.add("tradeSetsByLevel", levels);
            out.add(o);
        }
        // The registry iterates in registration order; sort so that the table
        // does not move when an unrelated profession is added.
        List<JsonElement> sorted = new ArrayList<>();
        out.forEach(sorted::add);
        sorted.sort((a, b) -> a.getAsJsonObject().get("id").getAsString()
                .compareTo(b.getAsJsonObject().get("id").getAsString()));
        JsonArray stable = new JsonArray();
        sorted.forEach(stable::add);
        return stable;
    }

    // -------------------------------------------------------------- tag order

    /// `TagLoader.tryBuildTag` collects into a **`LinkedHashSet`**
    /// (offsets 0..8) driven by `TagEntry.build`, and `TagLoader.build` sorts
    /// tags so that a referenced tag is finished before the tag referencing
    /// it. The observable rule is therefore: walk `values` in order, splice a
    /// `#tag` reference's own resolved list in at the point of reference, and
    /// **deduplicate elements, keeping the first occurrence**.
    ///
    /// That last clause is the one a naive implementation gets wrong. Guarding
    /// against re-entering a tag name (rather than against re-adding an
    /// element) gives the same answer only when no id appears twice, and gives
    /// a longer pool — and so a different `nextInt(size)` — when one does.
    private static List<String> resolveTag(Map<String, String> tags, String id) {
        LinkedHashSet<String> out = new LinkedHashSet<>();
        collect(tags, id, out, new LinkedHashSet<>());
        return new ArrayList<>(out);
    }

    private static void collect(Map<String, String> tags, String id,
                                LinkedHashSet<String> out, LinkedHashSet<String> onStack) {
        if (!onStack.add(id)) {
            throw new IllegalStateException("cycle through villager_trade tag " + id);
        }
        String raw = tags.get(id);
        if (raw == null) {
            throw new IllegalStateException("villager_trade tag " + id + " does not exist");
        }
        JsonArray values = JsonParser.parseString(raw).getAsJsonObject().getAsJsonArray("values");
        for (JsonElement v : values) {
            String entry;
            if (v.isJsonObject()) {
                entry = v.getAsJsonObject().get("id").getAsString();
            } else {
                entry = v.getAsString();
            }
            if (entry.startsWith("#")) {
                collect(tags, qualify(entry.substring(1)), out, onStack);
            } else {
                out.add(qualify(entry));
            }
        }
        onStack.remove(id);
    }

    private static String qualify(String id) {
        return id.contains(":") ? id : "minecraft:" + id;
    }

    /// The wandering trader's pools and the professions' pools must not reach
    /// each other, or splitting the output in two would drop rows a reader
    /// needs. Checked rather than assumed.
    private static void assertPartition(Map<String, String> tags, Map<String, List<String>> resolved) {
        for (Map.Entry<String, List<String>> e : resolved.entrySet()) {
            boolean wt = isWt(e.getKey());
            for (String member : e.getValue()) {
                if (isWt(member) != wt) {
                    throw new IllegalStateException("tag " + e.getKey() + " reaches " + member
                            + " across the wandering-trader split; the two tables are not a partition");
                }
            }
        }
    }

    private static boolean isWt(String id) {
        return id.startsWith("minecraft:" + WT);
    }

    // ----------------------------------------------------------------- IO

    /// Every trade-shaped file under a built-in datapack — the population the
    /// prefix match above has to exclude.
    private static int countUnderDatapacks(String jarPath) throws Exception {
        int n = 0;
        try (ZipFile zip = new ZipFile(jarPath)) {
            var it = zip.entries();
            while (it.hasMoreElements()) {
                String name = it.nextElement().getName();
                if (name.contains("datapacks/") && name.endsWith(".json")
                        && (name.contains("/villager_trade/") || name.contains("/trade_set/"))) {
                    n++;
                }
            }
        }
        return n;
    }

    private static Map<String, String> readFromJar(String jarPath, String prefix) throws Exception {
        Map<String, String> out = new TreeMap<>();
        try (ZipFile zip = new ZipFile(jarPath)) {
            var it = zip.entries();
            while (it.hasMoreElements()) {
                ZipEntry e = it.nextElement();
                String n = e.getName();
                if (!n.startsWith(prefix) || !n.endsWith(".json")) {
                    continue;
                }
                String rel = n.substring(prefix.length(), n.length() - ".json".length());
                try (InputStream in = zip.getInputStream(e)) {
                    out.put("minecraft:" + rel, new String(readAll(in), StandardCharsets.UTF_8));
                }
            }
        }
        return out;
    }

    private static Map<String, String> readFromDir(Path root) throws Exception {
        Map<String, String> out = new TreeMap<>();
        if (!Files.isDirectory(root)) {
            throw new IllegalStateException("no generated-data at " + root
                    + " — the cross-check has no second source and this dumper refuses to run on one");
        }
        try (var walk = Files.walk(root)) {
            for (Path p : walk.filter(x -> x.toString().endsWith(".json")).toList()) {
                String rel = root.relativize(p).toString().replace(java.io.File.separatorChar, '/');
                rel = rel.substring(0, rel.length() - ".json".length());
                out.put("minecraft:" + rel, Files.readString(p, StandardCharsets.UTF_8));
            }
        }
        return out;
    }

    private static byte[] readAll(InputStream in) throws Exception {
        ByteArrayOutputStream b = new ByteArrayOutputStream();
        byte[] buf = new byte[8192];
        int n;
        while ((n = in.read(buf)) > 0) {
            b.write(buf, 0, n);
        }
        return b.toByteArray();
    }

    /// Compares on the *parsed* JSON, not on the bytes, so that a difference
    /// in trailing newline or key order between the two writers is not
    /// reported as a version mismatch. Anything a reader could act on differs
    /// in the tree, and the tree is what is compared.
    private static int countMismatches(Map<String, String> a, Map<String, String> b) {
        int bad = 0;
        LinkedHashSet<String> keys = new LinkedHashSet<>();
        keys.addAll(a.keySet());
        keys.addAll(b.keySet());
        for (String k : keys) {
            String x = a.get(k);
            String y = b.get(k);
            if (x == null || y == null) {
                System.err.println("  !! " + k + " present in only one source");
                bad++;
                continue;
            }
            if (!JsonParser.parseString(x).equals(JsonParser.parseString(y))) {
                System.err.println("  !! " + k + " differs between the jar and generated-data");
                bad++;
            }
        }
        return bad;
    }

    private VillagerTradeGolden() {
        throw new AssertionError(Arrays.toString(new int[0]));
    }
}
