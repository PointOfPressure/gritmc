// LootGolden — vanilla's own loot tables, evaluated by vanilla, dumped as vectors.
//
// # Why this exists
//
// The 1,355 loot tables ship in the pinned jar as datapack JSON, so their
// *structure* is free. Their *behaviour* is not: what `explosion_decay` does,
// and above all the ORDER in which a seeded stream is drawn from, are bytecode
// facts that no amount of reading JSON reveals. A port that reproduces the
// structure and guesses the draw order produces plausible loot that is wrong,
// and no check built from the JSON alone could tell.
//
// So this dumper makes vanilla evaluate its own tables against a fixed seed and
// records exactly what came out. A Rust interpreter that reproduces these
// vectors has reproduced the draw order; one that does not has not, however
// well it reads the JSON.
//
// # The one construction that is not vanilla's
//
// `LootTable.getRandomItems(LootParams, long)` cannot be called here.
// `LootContext.Builder.create` dereferences `getLevel().getServer()` on its
// first two instructions — offsets 1 and 6, read with `javap -c` — so a harness
// without a running server NPEs before any loot is rolled, and there is no
// overload that skips it.
//
// `LootContext`'s own constructor takes exactly `(LootParams, RandomSource,
// HolderGetter.Provider)` and is private. This dumper reaches it reflectively
// and then calls the **public** `getRandomItemsRaw(LootContext, Consumer)`.
// Everything that decides an item is still vanilla's code on vanilla's data;
// the only thing bypassed is the server lookup that supplies the random source
// and the resolver, and both are supplied here explicitly and deterministically.
//
// # Failures are results, not omissions
//
// A condition or function that reaches for the level throws, because the level
// really is null. That is deliberate and it is the second measurement this file
// takes: **the set of tables that cannot be evaluated without a live level is
// exactly the set whose port is blocked on a subsystem rather than on an
// interpreter.** Every failure is recorded with its exception, never swallowed.
// A dumper that caught these and emitted an empty stack list would report a
// port as complete while it dropped every context-dependent drop on the floor —
// the ORACLE.md 5b fault, which this project has chased four times.
//
// Output: JSON on stdout. See `ci/loot-golden.sh`.

package net.minecraft.world.level.storage.loot;

import java.io.PrintStream;
import java.lang.reflect.Constructor;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;

import com.google.gson.JsonElement;
import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;

import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryOps;
import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.RandomSource;
import net.minecraft.util.context.ContextKeySet;
import net.minecraft.util.context.ContextMap;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.storage.loot.parameters.LootContextParamSets;
import net.minecraft.world.level.storage.loot.parameters.LootContextParams;
import net.minecraft.world.phys.Vec3;

public final class LootGolden {

    /// Seeds every table is rolled with. Four, not one: a single seed cannot
    /// distinguish an interpreter that draws in the right order from one that
    /// happens to land on the same item, and the tables with a `uniform` count
    /// need more than one sample before the count is evidence of anything.
    private static final long[] SEEDS = {0L, 1L, 4815162342L, -3L};

    /// Tools the block tables are rolled with. `match_tool` is the fourth most
    /// load-bearing opcode in the corpus (138 uses), and a dump taken bare-handed
    /// would record every silk-touch and every shears branch as "did not fire"
    /// while looking like full coverage.
    private static final String[] TOOLS = {"bare", "iron_pickaxe", "shears", "diamond_sword"};

    /// The stand-in level every roll is given; see `main`.
    private static net.minecraft.server.level.ServerLevel LEVEL;

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // THE WHOLE DATAPACK, LOADED THE WAY A DEDICATED SERVER LOADS IT — since
        // 2026-10-01. `WorldLoader.load` over `ServerPacksSource
        // .createVanillaTrustedRepository()` builds the layered registries,
        // binds EVERY tag (datapack registries included), loads the 1,355 loot
        // tables into `ReloadableServerRegistries`, and
        // `updateComponentsAndStaticRegistryTags` binds item components and
        // the static registries' tags — the two steps this file used to
        // reconstruct by hand (`DATA_COMPONENT_INITIALIZERS.build` and
        // `bindTags`), and which still could not reach the ENCHANTMENT
        // registry's tags or resolve a nested `loot_table` entry.
        //
        // What that bought, measured on the same run: the 31 cases that died
        // on `Tag minecraft:on_random_loot can't be dereferenced during
        // construction` now roll, and the nested-table entries this oracle
        // used to roll as EMPTY (the "oracle-blind" class the replay excluded)
        // resolve as vanilla resolves them.
        //
        // `LootContext.getLevel()` is read by a few functions for one thing,
        // `registryAccess()` (`enchant_with_levels`, an option-less
        // `enchant_randomly`). A real `ServerLevel` needs a running server, so
        // the level passed is an UNINITIALISED instance with exactly
        // `Level.registryAccess` set — any function that reads more of the
        // level than that (`exploration_map`'s chunk source) throws, and is
        // counted as a harness failure rather than recorded as a drop.
        Object[] layeredOut = new Object[1];
        var repo = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        var initConfig = new net.minecraft.server.WorldLoader.InitConfig(
                new net.minecraft.server.WorldLoader.PackConfig(
                        repo, net.minecraft.world.level.WorldDataConfiguration.DEFAULT, false, true),
                net.minecraft.commands.Commands.CommandSelection.DEDICATED,
                net.minecraft.server.permissions.LevelBasedPermissionSet.OWNER);
        java.util.concurrent.Executor direct = Runnable::run;
        net.minecraft.server.ReloadableServerResources resources =
                net.minecraft.server.WorldLoader.<Object, net.minecraft.server.ReloadableServerResources>load(
                        initConfig,
                        c -> new net.minecraft.server.WorldLoader.DataLoadOutput<>(null, c.datapackDimensions()),
                        (rm, r, layered, cookie) -> { layeredOut[0] = layered; return r; },
                        java.util.concurrent.ForkJoinPool.commonPool(), direct).get();
        resources.updateComponentsAndStaticRegistryTags();
        HolderLookup.Provider registries = resources.fullRegistries().lookup();
        java.lang.reflect.Field unsafeField = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        sun.misc.Unsafe unsafe = (sun.misc.Unsafe) unsafeField.get(null);
        LEVEL = (net.minecraft.server.level.ServerLevel) unsafe.allocateInstance(
                net.minecraft.server.level.ServerLevel.class);
        java.lang.reflect.Field registryAccessField =
                net.minecraft.world.level.Level.class.getDeclaredField("registryAccess");
        registryAccessField.setAccessible(true);
        registryAccessField.set(LEVEL,
                ((net.minecraft.core.LayeredRegistryAccess<?>) layeredOut[0]).compositeAccess());
        RegistryOps<JsonElement> ops = RegistryOps.create(JsonOps.INSTANCE, registries);

        // `LootContext(LootParams, RandomSource, HolderGetter.Provider)` — private.
        Constructor<LootContext> ctor = LootContext.class.getDeclaredConstructor(
                LootParams.class, RandomSource.class, HolderGetter.Provider.class);
        ctor.setAccessible(true);

        writeBlockLootMap();

        Map<String, JsonElement> sources = JarTables.read();
        // NOT System.out. `Bootstrap.bootStrap()` has already redirected it
        // through log4j, which re-emits every line with a
        // `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix — so a JSON document
        // written to stdout comes back as 1.3 million unparseable lines, and
        // an anchored grep over it matches nothing while exiting 1, which is
        // indistinguishable from a clean run that found nothing.
        String dest = System.getProperty("rustmc.loot.out");
        if (dest == null) throw new IllegalStateException("-Drustmc.loot.out is required");
        PrintStream out = new PrintStream(new java.io.BufferedOutputStream(
                java.nio.file.Files.newOutputStream(java.nio.file.Path.of(dest)), 1 << 20),
                false, StandardCharsets.UTF_8);

        out.println("{");
        out.println("  \"version\": \"" + detectVersion() + "\",");
        out.println("  \"tables_read\": " + sources.size() + ",");
        out.println("  \"seeds\": " + java.util.Arrays.toString(SEEDS) + ",");
        // Read by the replay: with the full datapack loaded, a nested
        // `loot_table` entry resolves, so it is compared rather than excluded.
        out.println("  \"oracle\": \"worldloader\",");
        // The exact block state each table was rolled against, so a port
        // reproduces THIS context rather than reconstructing one. Without it a
        // `block_state_property` condition on the Rust side would be comparing
        // against a default state it inferred, and an inference that happened
        // to agree would look like a passing port.
        out.print("  \"states\": {");
        boolean fs = true;
        for (String id : new TreeMap<>(sources).keySet()) {
            if (!id.startsWith("blocks/")) continue;
            BlockState st = blockStateFor(id);
            if (!fs) out.print(", ");
            fs = false;
            out.print("\"" + esc(id) + "\": {\"block\": \"" + esc(String.valueOf(BuiltInRegistries.BLOCK.getKey(st.getBlock()))) + "\", \"props\": {");
            boolean fp = true;
            for (var prop : st.getProperties()) {
                if (!fp) out.print(", ");
                fp = false;
                out.print("\"" + esc(prop.getName()) + "\": \"" + esc(stringify(st, prop)) + "\"");
            }
            out.print("}}");
        }
        out.println("},");
        out.println("  \"cases\": [");

        boolean first = true;
        int decoded = 0, evaluated = 0, failed = 0;
        Map<String, Integer> failureKinds = new TreeMap<>();

        for (Map.Entry<String, JsonElement> e : new TreeMap<>(sources).entrySet()) {
            String id = e.getKey();
            LootTable table;
            try {
                table = LootTable.DIRECT_CODEC.parse(ops, e.getValue()).getOrThrow();
                decoded++;
            } catch (Throwable t) {
                failed++;
                bump(failureKinds, "decode:" + t.getClass().getSimpleName());
                if (!first) out.println(",");
                first = false;
                out.print(caseJson(id, "-", 0L, null, "decode: " + brief(t)));
                continue;
            }

            ContextKeySet set = table.getParamSet();
            for (String tool : toolsFor(set)) {
                for (long seed : SEEDS) {
                    String err = null;
                    List<ItemStack> got = new ArrayList<>();
                    try {
                        ContextMap map = contextFor(set, id, tool);
                        LootParams params = new LootParams(LEVEL, map, Map.of(), 0.0F);
                        RandomSource random = RandomSource.create(seed);
                        LootContext ctx = ctor.newInstance(params, random, registries);
                        table.getRandomItemsRaw(ctx, got::add);
                        evaluated++;
                    } catch (Throwable t) {
                        Throwable c = t.getCause() != null ? t.getCause() : t;
                        err = brief(c);
                        failed++;
                        bump(failureKinds, "eval:" + c.getClass().getSimpleName());
                    }
                    if (!first) out.println(",");
                    first = false;
                    out.print(caseJson(id, tool, seed, err == null ? got : null, err));
                }
            }
        }
        out.println();
        out.println("  ],");
        writeSequences(out);
        writeFills(out, sources, ops, ctor, registries);
        writeEntityCases(out, sources, ops, ctor, registries);
        out.println("  \"decoded\": " + decoded + ",");
        out.println("  \"evaluated\": " + evaluated + ",");
        out.println("  \"failed\": " + failed + ",");
        out.print("  \"failure_kinds\": {");
        boolean f2 = true;
        for (Map.Entry<String, Integer> k : failureKinds.entrySet()) {
            if (!f2) out.print(", ");
            f2 = false;
            out.print("\"" + esc(k.getKey()) + "\": " + k.getValue());
        }
        out.println("}");
        out.println("}");
        out.flush();
        out.close();
    }

    /// Which block uses which loot table — the mapping the JSON cannot show.
    ///
    /// A loot table file called `blocks/stone.json` looks like it belongs to
    /// `minecraft:stone`, and 1,113 times out of 1,196 it does. But the mapping
    /// is a **field on `Block`**, set through `Properties.dropsLike(...)` or
    /// suppressed with `noLootTable()`, and neither leaves a trace in any JSON.
    /// Assuming the convention would silently give 61 blocks the wrong drops and
    /// 22 blocks drops they should not have — and every one of those would look
    /// exactly like a correct port, because the table it rolled really does
    /// exist and really does produce a plausible item.
    ///
    /// `BlockBehaviour.getLootTable()` is public and answers it directly:
    ///
    ///     1,196 blocks
    ///     1,113 follow `minecraft:blocks/<name>` exactly
    ///        61 point at another block's table
    ///        22 have no table at all
    ///     1,113 distinct tables referenced — the whole block corpus, exactly
    private static void writeBlockLootMap() throws Exception {
        String dest = System.getProperty("rustmc.blockmap.out");
        if (dest == null) return;
        PrintStream out = new PrintStream(java.nio.file.Files.newOutputStream(
                java.nio.file.Path.of(dest)), false, StandardCharsets.UTF_8);
        int total = 0, none = 0, conventional = 0, unconventional = 0;
        out.println("{");
        boolean first = true;
        for (Identifier id : new java.util.TreeSet<>(BuiltInRegistries.BLOCK.keySet())) {
            Block b = BuiltInRegistries.BLOCK.getValue(id);
            total++;
            var lt = b.getLootTable();
            String v = lt.map(k -> k.identifier().toString()).orElse(null);
            if (v == null) {
                none++;
            } else if (v.equals(id.getNamespace() + ":blocks/" + id.getPath())) {
                conventional++;
            } else {
                unconventional++;
            }
            if (!first) out.println(",");
            first = false;
            out.print("  \"" + esc(id.toString()) + "\": " + (v == null ? "null" : "\"" + esc(v) + "\""));
        }
        out.println();
        out.println("}");
        out.close();
        Bootstrap.STDOUT.println("block-loot-map: " + total + " blocks, " + conventional
                + " conventional, " + unconventional + " pointing at another block's table, "
                + none + " with no table");
    }

    private static void bump(Map<String, Integer> m, String k) {
        m.merge(k, 1, Integer::sum);
    }

    /// Only the block-shaped param sets vary by tool; rolling a chest table four
    /// times with four different tools would quadruple the file and say nothing.
    private static String[] toolsFor(ContextKeySet set) {
        return set == LootContextParamSets.BLOCK || set == LootContextParamSets.SHEARING
                ? TOOLS : new String[]{"bare"};
    }

    private static ContextMap contextFor(ContextKeySet set, String id, String tool) {
        ContextMap.Builder b = new ContextMap.Builder();
        // Only what this set ALLOWS. `create(set)` rejects a map carrying a key
        // the set does not list — "Parameters not allowed in this parameter
        // set" — so handing every table an origin and a block state failed 968
        // cases on the chest and entity tables for a reason that had nothing to
        // do with loot. `allowed()` is read off the set itself rather than
        // hardcoded per param set, so a set this file has never heard of is
        // filled correctly rather than skipped.
        var allowed = set.allowed();
        if (allowed.contains(LootContextParams.ORIGIN))
            b.withParameter(LootContextParams.ORIGIN, new Vec3(0.5, 64.0, 0.5));
        if (allowed.contains(LootContextParams.BLOCK_STATE))
            b.withParameter(LootContextParams.BLOCK_STATE, blockStateFor(id));
        if (allowed.contains(LootContextParams.TOOL))
            b.withParameter(LootContextParams.TOOL, toolStack(tool));
        return b.create(set);
    }

    private static BlockState blockStateFor(String id) {
        if (id.startsWith("blocks/")) {
            Identifier key = Identifier.tryParse("minecraft:" + id.substring("blocks/".length()));
            if (key != null) {
                Block block = BuiltInRegistries.BLOCK.getValue(key);
                if (block != null && block != Blocks.AIR) return block.defaultBlockState();
            }
        }
        return Blocks.STONE.defaultBlockState();
    }

    private static ItemStack toolStack(String tool) {
        return switch (tool) {
            case "iron_pickaxe" -> new ItemStack(Items.IRON_PICKAXE);
            case "shears" -> new ItemStack(Items.SHEARS);
            case "diamond_sword" -> new ItemStack(Items.DIAMOND_SWORD);
            default -> ItemStack.EMPTY;
        };
    }

    private static String caseJson(String id, String tool, long seed, List<ItemStack> stacks, String err) {
        StringBuilder sb = new StringBuilder();
        sb.append("    {\"table\": \"").append(esc(id)).append("\", \"tool\": \"").append(esc(tool))
          .append("\", \"seed\": ").append(seed);
        if (err != null) {
            sb.append(", \"error\": \"").append(esc(err)).append("\"}");
            return sb.toString();
        }
        sb.append(", \"stacks\": [");
        for (int i = 0; i < stacks.size(); i++) {
            ItemStack s = stacks.get(i);
            if (i > 0) sb.append(", ");
            Identifier item = BuiltInRegistries.ITEM.getKey(s.getItem());
            sb.append("{\"item\": \"").append(esc(String.valueOf(item)))
              .append("\", \"count\": ").append(s.getCount()).append("}");
        }
        sb.append("]}");
        return sb.toString();
    }

    @SuppressWarnings("unchecked")
    private static <T extends Comparable<T>> String stringify(
            BlockState st, net.minecraft.world.level.block.state.properties.Property<?> prop) {
        var p = (net.minecraft.world.level.block.state.properties.Property<T>) prop;
        return p.getName(st.getValue(p));
    }

    private static String brief(Throwable t) {
        String m = t.getMessage();
        String s = t.getClass().getSimpleName() + (m == null ? "" : ": " + m);
        return s.length() > 240 ? s.substring(0, 240) : s;
    }

    private static String esc(String s) {
        return s.replace("\\", "\\\\").replace("\"", "\\\"").replace("\n", " ").replace("\r", " ");
    }

    private static String detectVersion() {
        String v = System.getProperty("rustmc.version");
        return v == null ? "unknown" : v;
    }

    /// Reads the loot tables straight out of the jar this class is running from.
    ///
    /// The outer `server.jar` is a bundler and holds nothing but the nested
    /// `META-INF/versions/<v>/server-<v>.jar`; grepping the outer one for
    /// `loot_table` returns zero. Here the nested jar is already the classpath,
    /// so the tables are ordinary classpath resources under `data/minecraft/`.
    static final class JarTables {
        static Map<String, JsonElement> read() throws Exception {
            String listing = System.getProperty("rustmc.loot.dir");
            Map<String, JsonElement> out = new TreeMap<>();
            java.nio.file.Path dir = java.nio.file.Path.of(listing);
            try (var walk = java.nio.file.Files.walk(dir)) {
                for (java.nio.file.Path p : walk.filter(x -> x.toString().endsWith(".json")).toList()) {
                    String rel = dir.relativize(p).toString();
                    rel = rel.substring(0, rel.length() - ".json".length());
                    try (var r = java.nio.file.Files.newBufferedReader(p, StandardCharsets.UTF_8)) {
                        out.put(rel, JsonParser.parseReader(r));
                    }
                }
            }
            return out;
        }
    }


    // ---------------------------------------------------------------- sequences
    //
    // `RandomSequences.get(id, worldSeed)` straight from vanilla, no
    // re-implementation: a fresh store per (seed, id), then the first eight
    // `nextLong()`s. This is the generator a loot table with no explicit seed
    // draws from (`LootContext.Builder.create`'s second arm), and the vector
    // pins both halves of its seed — the world seed and the md5 of the id.
    private static final long[] WORLD_SEEDS = {0L, 20260803L, -7L};
    private static final String[] SEQUENCE_IDS = {
        "minecraft:chests/simple_dungeon", "minecraft:entities/pig",
        "minecraft:chests/village/village_weaponsmith", "minecraft:entities/zombie"};

    private static void writeSequences(PrintStream out) {
        out.println("  \"sequences\": [");
        boolean first = true;
        for (long ws : WORLD_SEEDS) {
            for (String id : SEQUENCE_IDS) {
                net.minecraft.world.RandomSequences seqs = new net.minecraft.world.RandomSequences();
                RandomSource r = seqs.get(Identifier.parse(id), ws);
                StringBuilder sb = new StringBuilder();
                for (int i = 0; i < 8; i++) {
                    if (i > 0) sb.append(", ");
                    sb.append(r.nextLong());
                }
                if (!first) out.println(",");
                first = false;
                out.print("    {\"world_seed\": " + ws + ", \"id\": \"" + esc(id) + "\", \"longs\": [" + sb + "]}");
            }
        }
        out.println();
        out.println("  ],");
    }

    // -------------------------------------------------------------------- fills
    //
    // `LootTable.fill(Container, LootParams, long)` cannot be called whole in
    // this harness: its context builder reaches `params.getLevel().getServer()`
    // and the stack splitter reaches `level.enabledFeatures()`, and there is no
    // level. So the three parts are called separately, two of them as
    // vanilla's own private methods by reflection:
    //
    //   * the roll: `getRandomItemsRaw(LootContext, Consumer)`, as above;
    //   * the splitter: `createStackSplitter`'s lambda, restated here because
    //     its only use of the level is the feature-flag test, which every
    //     shipped table passes — count below `getMaxStackSize()` passes
    //     through (the empty stack included), anything else is cut into
    //     `min(max, left)` pieces;
    //   * `getAvailableSlots(Container, RandomSource)` and
    //     `shuffleAndSplitItems(ObjectArrayList, int, RandomSource)` —
    //     vanilla's own bytecode, on the SAME generator the roll advanced;
    //   * then `fill`'s placement loop, restated: pop the last slot, write.
    //
    // Two generator arms, because `LootContext.Builder.create` has two that a
    // chest reaches: an explicit `LootTableSeed` (`RandomSource.create(seed)`),
    // and seed 0, which draws from the table's named random sequence. The
    // sequence arm fills the SAME chest twice from one store, so the vector
    // pins that the store is advanced rather than re-created.
    private static final long[] FILL_SEEDS = {1L, 4815162342L, -3L};

    private static void writeFills(PrintStream out, Map<String, JsonElement> sources,
                                   RegistryOps<JsonElement> ops, Constructor<LootContext> ctor,
                                   HolderLookup.Provider registries) throws Exception {
        java.lang.reflect.Method slotsM = LootTable.class.getDeclaredMethod(
                "getAvailableSlots", net.minecraft.world.Container.class, RandomSource.class);
        slotsM.setAccessible(true);
        java.lang.reflect.Method splitM = LootTable.class.getDeclaredMethod(
                "shuffleAndSplitItems", it.unimi.dsi.fastutil.objects.ObjectArrayList.class, int.class, RandomSource.class);
        splitM.setAccessible(true);
        out.println("  \"fills\": [");
        boolean first = true;
        int ok = 0, bad = 0;
        for (Map.Entry<String, JsonElement> e : new TreeMap<>(sources).entrySet()) {
            String id = e.getKey();
            if (!id.startsWith("chests/")) continue;
            LootTable table;
            try {
                table = LootTable.DIRECT_CODEC.parse(ops, e.getValue()).getOrThrow();
            } catch (Throwable t) {
                continue;
            }
            List<Object[]> arms = new ArrayList<>();
            for (long seed : FILL_SEEDS) arms.add(new Object[]{"seed", seed, RandomSource.create(seed), 1});
            java.lang.reflect.Field seqF = LootTable.class.getDeclaredField("randomSequence");
            seqF.setAccessible(true);
            @SuppressWarnings("unchecked")
            Optional<Identifier> seq = (Optional<Identifier>) seqF.get(table);
            if (seq.isPresent()) {
                net.minecraft.world.RandomSequences store = new net.minecraft.world.RandomSequences();
                arms.add(new Object[]{"sequence", 20260803L, store.get(seq.get(), 20260803L), 2});
            }
            for (Object[] arm : arms) {
                RandomSource random = (RandomSource) arm[2];
                int times = (Integer) arm[3];
                for (int pass = 0; pass < times; pass++) {
                    String err = null;
                    net.minecraft.world.SimpleContainer box = new net.minecraft.world.SimpleContainer(27);
                    try {
                        ContextMap.Builder b = new ContextMap.Builder();
                        b.withParameter(LootContextParams.ORIGIN, new Vec3(0.5, 64.0, 0.5));
                        LootParams params = new LootParams(LEVEL, b.create(LootContextParamSets.CHEST), Map.of(), 0.0F);
                        LootContext ctx = ctor.newInstance(params, random, registries);
                        List<ItemStack> raw = new ArrayList<>();
                        table.getRandomItemsRaw(ctx, raw::add);
                        it.unimi.dsi.fastutil.objects.ObjectArrayList<ItemStack> items = new it.unimi.dsi.fastutil.objects.ObjectArrayList<>();
                        for (ItemStack s : raw) {
                            if (s.getCount() < s.getMaxStackSize()) {
                                items.add(s);
                            } else {
                                int left = s.getCount();
                                while (left > 0) {
                                    ItemStack c = s.copyWithCount(Math.min(s.getMaxStackSize(), left));
                                    left -= c.getCount();
                                    items.add(c);
                                }
                            }
                        }
                        @SuppressWarnings("unchecked")
                        List<Integer> slots = (List<Integer>) slotsM.invoke(table, box, random);
                        splitM.invoke(table, items, slots.size(), random);
                        for (ItemStack s : items) {
                            if (slots.isEmpty()) break;
                            box.setItem(slots.remove(slots.size() - 1), s.isEmpty() ? ItemStack.EMPTY : s);
                        }
                        ok++;
                    } catch (Throwable t) {
                        Throwable c = t.getCause() != null ? t.getCause() : t;
                        err = brief(c);
                        bad++;
                    }
                    if (!first) out.println(",");
                    first = false;
                    StringBuilder sb = new StringBuilder();
                    sb.append("    {\"table\": \"").append(esc(id)).append("\", \"arm\": \"").append(arm[0])
                      .append("\", \"seed\": ").append(arm[1]).append(", \"pass\": ").append(pass);
                    if (err != null) {
                        sb.append(", \"error\": \"").append(esc(err)).append("\"}");
                    } else {
                        sb.append(", \"slots\": [");
                        boolean fs = true;
                        for (int i = 0; i < box.getContainerSize(); i++) {
                            ItemStack s = box.getItem(i);
                            if (s.isEmpty()) continue;
                            if (!fs) sb.append(", ");
                            fs = false;
                            sb.append("[").append(i).append(", \"").append(esc(String.valueOf(BuiltInRegistries.ITEM.getKey(s.getItem()))))
                              .append("\", ").append(s.getCount()).append("]");
                        }
                        sb.append("]}");
                    }
                    out.print(sb);
                }
            }
        }
        out.println();
        out.println("  ],");
        // `getMaxStackSize()` for every item that does not stack to 64, which
        // is what the splitter reads. Emitted here so the replay needs no
        // second table; an item absent from this map stacks to 64.
        out.print("  \"max_stack\": {");
        boolean fm = true;
        for (Identifier key : new java.util.TreeSet<>(BuiltInRegistries.ITEM.keySet())) {
            int m = new ItemStack(BuiltInRegistries.ITEM.getValue(key)).getMaxStackSize();
            if (m == 64) continue;
            if (!fm) out.print(", ");
            fm = false;
            out.print("\"" + esc(key.toString()) + "\": " + m);
        }
        out.println("},");
        Bootstrap.STDOUT.println("loot-fills: " + ok + " fills taken, " + bad + " refused by the harness");
    }


    // ------------------------------------------------------------ entity cases
    //
    // Every `entities/*` table failed above on `Missing required parameters:
    // [this_entity, …]`, because `LootContextParamSets.ENTITY` requires an
    // entity and this harness has no level to make one in. Those failures are
    // the harness's, not the tables', so the tables are rolled again here
    // under a parameter set that REQUIRES nothing and allows the ENTITY set's
    // keys, with ORIGIN alone supplied.
    //
    // That is a real context and not a convenience: it is what every
    // optional-parameter read in the vocabulary sees when the parameter is
    // absent — `entity_properties` gets a null entity (`EntityPredicate
    // .matches` offset 1: `ifnonnull`, else false, no draw);
    // `killed_by_player` gets `hasParameter(LAST_DAMAGE_PLAYER)` false;
    // `random_chance_with_enchanted_bonus` and `enchanted_count_increase` see
    // no `ATTACKING_ENTITY` and fall to level 0; `damage_source_properties`
    // sees no source and answers false. A mob killed by `/kill` has an entity
    // and a source but no attacker and no player, so for every table whose
    // entity predicates are false of a live, unburning, adult mob these
    // vectors are exactly its drops.
    private static void writeEntityCases(PrintStream out, Map<String, JsonElement> sources,
                                         RegistryOps<JsonElement> ops, Constructor<LootContext> ctor,
                                         HolderLookup.Provider registries) {
        ContextKeySet open = new ContextKeySet.Builder()
                .optional(LootContextParams.THIS_ENTITY)
                .optional(LootContextParams.LAST_DAMAGE_PLAYER)
                .optional(LootContextParams.DAMAGE_SOURCE)
                .optional(LootContextParams.ATTACKING_ENTITY)
                .optional(LootContextParams.DIRECT_ATTACKING_ENTITY)
                .optional(LootContextParams.ORIGIN)
                .build();
        out.println("  \"entity_cases\": [");
        boolean first = true;
        int ok = 0, bad = 0;
        for (Map.Entry<String, JsonElement> e : new TreeMap<>(sources).entrySet()) {
            String id = e.getKey();
            if (!id.startsWith("entities/")) continue;
            LootTable table;
            try {
                table = LootTable.DIRECT_CODEC.parse(ops, e.getValue()).getOrThrow();
            } catch (Throwable t) {
                continue;
            }
            // The four LegacyRandomSource seeds, then the table's own random
            // sequence at world seed 20260803 rolled TWICE from one store —
            // seed 0 is what `dropFromLootTable` passes for a mob with no
            // `DeathLootTableSeed`, so this is the arm a real death takes.
            List<Object[]> arms = new ArrayList<>();
            for (long seed : SEEDS) arms.add(new Object[]{seed, RandomSource.create(seed)});
            try {
                java.lang.reflect.Field seqF = LootTable.class.getDeclaredField("randomSequence");
                seqF.setAccessible(true);
                @SuppressWarnings("unchecked")
                Optional<Identifier> seq = (Optional<Identifier>) seqF.get(table);
                if (seq.isPresent()) {
                    RandomSource r = new net.minecraft.world.RandomSequences().get(seq.get(), 20260803L);
                    arms.add(new Object[]{"sequence:0", r});
                    arms.add(new Object[]{"sequence:1", r});
                }
            } catch (ReflectiveOperationException roe) {
                throw new IllegalStateException(roe);
            }
            for (Object[] arm : arms) {
                String err = null;
                List<ItemStack> got = new ArrayList<>();
                try {
                    ContextMap.Builder b = new ContextMap.Builder();
                    b.withParameter(LootContextParams.ORIGIN, new Vec3(0.5, 64.0, 0.5));
                    LootParams params = new LootParams(LEVEL, b.create(open), Map.of(), 0.0F);
                    LootContext ctx = ctor.newInstance(params, (RandomSource) arm[1], registries);
                    table.getRandomItemsRaw(ctx, got::add);
                    ok++;
                } catch (Throwable t) {
                    Throwable c = t.getCause() != null ? t.getCause() : t;
                    err = brief(c);
                    bad++;
                }
                if (!first) out.println(",");
                first = false;
                if (arm[0] instanceof Long seed) {
                    out.print(caseJson(id, "bare", seed, err == null ? got : null, err));
                } else {
                    // `"seed": 0` plus an `arm`: the replay keys the store by table.
                    String c = caseJson(id, "bare", 0L, err == null ? got : null, err);
                    out.print(c.substring(0, c.length() - 1) + ", \"arm\": \"" + arm[0] + "\", \"world_seed\": 20260803}");
                }
            }
        }
        out.println();
        out.println("  ],");
        Bootstrap.STDOUT.println("loot-entity-cases: " + ok + " rolled with no entity, " + bad + " refused by the harness");
    }

    private LootGolden() {}
}
