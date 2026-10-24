// AdvancementGolden — vanilla 26.2's 1,688 advancement documents, as vanilla
// itself loads them, keyed by resource id.
//
// # What this is
//
// `ServerAdvancementManager.apply` is four steps: parse every
// `data/<ns>/advancement/**.json` through `Advancement.CODEC`, build an
// `AdvancementTree` with `addAll`, run `TreeNodePosition.run` on every root
// whose advancement has a `display`, and keep the map. This dumper does
// exactly those four steps against the pinned jar and writes what they
// produced. Nothing is reconstructed: the `document` field of every entry is
// the datapack's own JSON, byte for byte, and every entry is also PARSED
// through `Advancement.CODEC` first, so a document this table carries is one
// vanilla accepts.
//
// It carries the source rather than a re-encoding, and that is a measured
// choice rather than a shortcut. `Advancement.CODEC.encodeStart` refuses
// `adventure/adventuring_time`: 55 biome holders decoded through
// `VanillaRegistries.createLookup()` come back `Element Reference{...} is not
// valid in current registry set` on the way out, from `HolderSetCodec`'s
// owner check. Encoding is therefore not available here at any cost this item
// would pay, and the DERIVED value that a re-encode would have supplied —
// `requirements` when the document omits it — is emitted separately from
// `a.requirements()`, which is vanilla's own parsed object.
//
// # Why the wire bytes are captured rather than described
//
// `AdvancementHolder.STREAM_CODEC` runs `Identifier` then
// `Advancement.write`, and `Advancement.write` runs
// `DisplayInfo.STREAM_CODEC`, which is `ComponentSerialization.TRUSTED_STREAM_CODEC`
// twice (a title and a description, both NBT component trees) and then
// `ItemStackTemplate.STREAM_CODEC` (an item registry id and a
// `DataComponentPatch`). Reimplementing that tree in Rust to emit a packet
// whose only consumer is a client would be a second component protocol, not
// an advancement engine. So `holder_wire` is the exact byte string vanilla's
// own encoder produces for that advancement, hex, and the Rust side
// concatenates the ones it needs. Same argument, same shape, as the captured
// `RecipeDisplayEntry` bytes in `crates/rustmc-server/src/recipe_book.rs`.
//
// # THE TWO ORDERS THIS DUMPER EMITS AND DOES NOT TRUST
//
// `AdvancementNode.children` is an `it.unimi.dsi.fastutil.objects.ReferenceOpenHashSet`
// — an IDENTITY hash set. `AdvancementTree.roots` is an
// `ObjectLinkedOpenHashSet` fed from an `ImmutableMap` built in resource-scan
// order. Neither is a function of the data, so neither is reproducible across
// JVM runs, and `TreeNodePosition.run` reads `children()` to lay a tree out —
// which means `DisplayInfo.getX()/getY()`, and therefore `holder_wire`, may
// move between two runs of the same jar.
//
// That is a claim about vanilla, so it is measured rather than asserted:
// `children_observed` and `roots_observed` are written to a SEPARATE file
// (`-Drustmc.adv.orders.out`), `ci/advancements-golden.sh` runs this dumper
// twice into a fresh directory and diffs both files, and the committed table
// carries only what survived that diff. Every list in the committed table is
// sorted by resource id.
package net.minecraft.advancements;

import com.google.gson.JsonElement;
import com.mojang.serialization.JsonOps;
import io.netty.buffer.Unpooled;
import java.io.IOException;
import java.io.OutputStreamWriter;
import java.io.Writer;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.stream.Stream;

import net.minecraft.SharedConstants;
import net.minecraft.advancements.triggers.Criterion;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryOps;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.util.StrictJsonParser;

public final class AdvancementGolden {
    private AdvancementGolden() {}

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PrintStream log = Bootstrap.STDOUT;

        Path advDir = Paths.get(System.getProperty("rustmc.adv.dir"));
        Path out = Paths.get(System.getProperty("rustmc.adv.out"));
        Path ordersOut = Paths.get(System.getProperty("rustmc.adv.orders.out"));
        String version = System.getProperty("rustmc.version", "unknown");

        // TWO REGISTRY VIEWS, AND THEY ARE NOT INTERCHANGEABLE.
        //
        // Parsing needs the WORLDGEN registries: `adventure/adventuring_time`
        // names 55 biomes inside a `LocationPredicate`, and a `HolderSet`
        // codec with no biome registry behind it cannot resolve
        // `"minecraft:badlands"` as a holder, falls through to its list
        // branch, and reports `Not a json array` — which reads like a
        // malformed document and is a missing registry.
        // `VanillaRegistries.createLookup()` is the only view here that has
        // them.
        //
        // Encoding needs a `RegistryAccess`, which the lookup is not, and
        // only reaches the ITEM registry (through `ItemStackTemplate`), which
        // is built-in and lives in `RegistryLayer.STATIC`.
        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        RegistryAccess.Frozen access = RegistryLayer.createRegistryAccess().compositeAccess();
        RegistryOps<JsonElement> ops = RegistryOps.create(JsonOps.INSTANCE, lookup);

        // Parse, in id order. `ServerAdvancementManager` parses in resource
        // scan order; the ORDER of parsing changes nothing about any parsed
        // object, only the insertion order of the map it builds, which this
        // dumper does not publish (see the header).
        Map<Identifier, Advancement> parsed = new LinkedHashMap<>();
        Map<Identifier, String> source = new LinkedHashMap<>();
        List<Path> files = new ArrayList<>();
        try (Stream<Path> walk = Files.walk(advDir)) {
            walk.filter(p -> p.toString().endsWith(".json")).forEach(files::add);
        }
        files.sort(Comparator.comparing(Path::toString));
        // THE POSITIVE CONTROL FOR THE TWO ORDERS THIS DUMPER DOES NOT TRUST.
        //
        // A real server parses in resource-SCAN order, which is a property of
        // the pack rather than of the data, and this dumper parses in id
        // order. If `TreeNodePosition`'s layout — and therefore
        // `DisplayInfo.x/y`, and therefore `holder_wire` — depends on the
        // order the advancements were constructed in, then this table's wire
        // bytes are not the ones a real vanilla server sends, and every
        // comparison built on them measures the dumper.
        //
        // `-Drustmc.adv.shuffle=<seed>` permutes the parse order with that
        // seed and changes nothing else. `ci/advancements-golden.sh` runs a
        // third JVM with a shuffle and compares its table to the unshuffled
        // one: an equal table is the control passing, a differing one is the
        // finding.
        String shuffleSeed = System.getProperty("rustmc.adv.shuffle");
        if (shuffleSeed != null) {
            java.util.Collections.shuffle(files, new java.util.Random(Long.parseLong(shuffleSeed)));
        }
        for (Path p : files) {
            String rel = advDir.relativize(p).toString();
            Identifier id = Identifier.fromNamespaceAndPath(
                    "minecraft", rel.substring(0, rel.length() - 5).replace('\\', '/'));
            JsonElement json;
            try (var r = Files.newBufferedReader(p, StandardCharsets.UTF_8)) {
                json = StrictJsonParser.parse(r);
            }
            Advancement adv = Advancement.CODEC.parse(ops, json).getOrThrow(
                    s -> new IllegalStateException(id + ": " + s));
            parsed.put(id, adv);
            source.put(id, json.toString());
        }
        if (files.size() < 1000) {
            throw new IllegalStateException(
                    "only " + files.size() + " advancement documents under " + advDir
                            + " — a low count here is an instrument failure, not a result");
        }

        // `ServerAdvancementManager.apply`, step 2.
        List<AdvancementHolder> holders = new ArrayList<>();
        parsed.forEach((id, adv) -> holders.add(new AdvancementHolder(id, adv)));
        AdvancementTree tree = new AdvancementTree();
        tree.addAll(holders);

        // STEP 3 IS DELIBERATELY NOT RUN, AND THE FLOATS IT WRITES ARE PINNED
        // TO ZERO INSTEAD.
        //
        // `apply` runs `TreeNodePosition.run(root)` on every root with a
        // display, which lays the tree out by walking
        // `AdvancementNode.children` — a `ReferenceOpenHashSet`, identity
        // hashed — and writes `DisplayInfo.x/y`, which
        // `Advancement.write` puts on the wire. That makes those two floats,
        // and therefore `holder_wire`, non-reproducible:
        //
        //   * permuting the parse order and nothing else moves 122 of 126
        //     `y` values and 120 of 126 `holder_wire` strings (this script's
        //     third run, `-Drustmc.adv.shuffle`);
        //   * and it is not even stable between two IDENTICAL runs. Three
        //     consecutive runs on 2026-09-05 reproduced; a fourth, under
        //     `ci/gen-gameplay-tables.sh`, did not, and this dumper refused
        //     to write the table — correctly. An identity hash is a function
        //     of allocation, and allocation is a function of the machine.
        //
        // A table that cannot be regenerated is worse than a missing field:
        // `ci/t2-gameplay.sh` stamps every generated table and would flap.
        // So `setLocation(0, 0)` — a public setter on `DisplayInfo` — pins
        // both floats and `TreeNodePosition` is not run at all. Every other
        // byte of `holder_wire` is a function of the datapack.
        //
        // What is lost is where an advancement's icon sits in the client's
        // tree view. What is NOT lost is any server-side behaviour: nothing
        // in `PlayerAdvancements`, `AdvancementProgress` or the visibility
        // evaluator reads x or y. The divergence is stated in
        // `crates/rustmc-server/src/advancements.rs` and in
        // `docs/FINDINGS.md` rather than left to be found.
        for (Advancement a : parsed.values()) {
            a.display().ifPresent(d -> d.setLocation(0.0F, 0.0F));
        }

        // The observed, per-JVM orders, into their own file.
        StringBuilder orders = new StringBuilder();
        orders.append("{\n  \"roots_observed\": [");
        boolean first = true;
        for (AdvancementNode r : tree.roots()) {
            if (!first) orders.append(", ");
            first = false;
            orders.append('"').append(r.holder().id()).append('"');
        }
        orders.append("],\n  \"children_observed\": {\n");
        List<AdvancementNode> nodes = new ArrayList<>(tree.nodes());
        nodes.sort(Comparator.comparing(n -> n.holder().id().toString()));
        first = true;
        for (AdvancementNode n : nodes) {
            List<String> kids = new ArrayList<>();
            for (AdvancementNode c : n.children()) {
                kids.add(c.holder().id().toString());
            }
            if (kids.isEmpty()) continue;
            if (!first) orders.append(",\n");
            first = false;
            orders.append("    \"").append(n.holder().id()).append("\": [");
            for (int i = 0; i < kids.size(); i++) {
                if (i > 0) orders.append(", ");
                orders.append('"').append(kids.get(i)).append('"');
            }
            orders.append(']');
        }
        orders.append("\n  }\n}\n");
        Files.writeString(ordersOut, orders.toString(), StandardCharsets.UTF_8);

        // The table. Every map is a TreeMap and every list is sorted by id.
        Map<String, Object> table = new TreeMap<>();
        StringBuilder sb = new StringBuilder();
        sb.append("{\n");
        sb.append("  \"version\": \"").append(version).append("\",\n");
        sb.append("  \"count\": ").append(holders.size()).append(",\n");
        sb.append("  \"advancements\": {\n");

        List<AdvancementHolder> sorted = new ArrayList<>(holders);
        sorted.sort(Comparator.comparing(h -> h.id().toString()));
        List<String> unencodable = new ArrayList<>();
        int nWithDisplay = 0;
        int nRecipeRewards = 0;
        for (int i = 0; i < sorted.size(); i++) {
            AdvancementHolder h = sorted.get(i);
            Advancement a = h.value();
            // ONE ADVANCEMENT'S ICON CAN REFUSE TO ENCODE, AND THAT IS A
            // GAP TO COUNT, NOT AN ABORT.
            //
            // `DisplayInfo.serializeToNetwork` runs `ItemStackTemplate`'s
            // `DataComponentPatch` encoder, which resolves a component's
            // value through `registryAccess`. An icon carrying a
            // `minecraft:banner_patterns` component therefore needs the
            // `minecraft:banner_pattern` registry, and no `RegistryAccess`
            // this dumper can build without a `ResourceManager` over the
            // datapack holds it: `RegistryLayer.createRegistryAccess()` has
            // an EMPTY worldgen layer and `VanillaRegistries.createLookup()`
            // is a `HolderLookup.Provider`, not a `RegistryAccess`.
            //
            // So the failure is caught per advancement and the entry's
            // `holder_wire` is `null`. A null is a gap the Rust side cannot
            // send — the same shape as `RecipeUnlocks::hold` returning `None`
            // for an item its table does not name — and the count is printed,
            // so a version bump that turns two nulls into two hundred is
            // visible rather than silent.
            String wireHex;
            try {
                RegistryFriendlyByteBuf buf =
                        new RegistryFriendlyByteBuf(Unpooled.buffer(), access);
                AdvancementHolder.STREAM_CODEC.encode(buf, h);
                byte[] wire = new byte[buf.readableBytes()];
                buf.getBytes(0, wire);
                wireHex = hex(wire);
            } catch (RuntimeException e) {
                wireHex = null;
                unencodable.add(h.id() + " (" + e.getClass().getSimpleName() + ": "
                        + e.getMessage() + ")");
            }

            sb.append("    \"").append(h.id()).append("\": {");
            sb.append("\"document\": ").append(source.get(h.id()));
            sb.append(", \"requirements\": [");
            List<List<String>> req = a.requirements().requirements();
            for (int r = 0; r < req.size(); r++) {
                if (r > 0) sb.append(", ");
                sb.append('[');
                for (int c = 0; c < req.get(r).size(); c++) {
                    if (c > 0) sb.append(", ");
                    sb.append('"').append(req.get(r).get(c)).append('"');
                }
                sb.append(']');
            }
            sb.append("], \"criteria_triggers\": {");
            List<String> critNames = new ArrayList<>(a.criteria().keySet());
            critNames.sort(Comparator.naturalOrder());
            for (int c = 0; c < critNames.size(); c++) {
                if (c > 0) sb.append(", ");
                Criterion<?> crit = a.criteria().get(critNames.get(c));
                sb.append('"').append(critNames.get(c)).append("\": \"")
                        .append(triggerId(crit)).append('"');
            }
            sb.append('}');
            sb.append(", \"parent\": ")
                    .append(a.parent().map(p -> "\"" + p + "\"").orElse("null"));
            sb.append(", \"sends_telemetry_event\": ").append(a.sendsTelemetryEvent());
            sb.append(", \"experience\": ").append(a.rewards().experience());
            sb.append(", \"reward_recipes\": [");
            for (int r = 0; r < a.rewards().recipes().size(); r++) {
                if (r > 0) sb.append(", ");
                sb.append('"').append(a.rewards().recipes().get(r).identifier()).append('"');
            }
            sb.append("], \"reward_loot\": [");
            for (int r = 0; r < a.rewards().loot().size(); r++) {
                if (r > 0) sb.append(", ");
                sb.append('"').append(a.rewards().loot().get(r).identifier()).append('"');
            }
            sb.append("], \"reward_function\": ")
                    .append(a.rewards().function().isPresent() ? "true" : "false");
            if (a.display().isPresent()) {
                nWithDisplay++;
                DisplayInfo d = a.display().get();
                sb.append(", \"display\": {\"type\": \"").append(d.getType().name())
                        .append("\", \"show_toast\": ").append(d.shouldShowToast())
                        .append(", \"announce_chat\": ").append(d.shouldAnnounceChat())
                        .append(", \"hidden\": ").append(d.isHidden())
                        .append('}');
            } else {
                sb.append(", \"display\": null");
            }
            if (!a.rewards().recipes().isEmpty()) nRecipeRewards++;
            sb.append(", \"holder_wire\": ")
                    .append(wireHex == null ? "null" : "\"" + wireHex + "\"");
            sb.append('}');
            if (i + 1 < sorted.size()) sb.append(',');
            sb.append('\n');
        }
        sb.append("  },\n");

        // VANILLA'S OWN ANSWER FOR THE ONE CONTAINER THIS PORT REPRODUCES.
        //
        // `PlayerAdvancements.flushDirty` builds the packet's `added` and
        // `removed` sets and its progress map as plain `java.util.HashSet`s
        // and `HashMap`s, so the order those three reach the wire in is
        // `java.util.HashMap`'s. `crates/rustmc-server/src/advancements/jhash.rs`
        // reimplements it, and a reimplementation with no vector table is
        // the thing `openhash.rs`'s header refuses.
        //
        // Emitted per visible tree, under TWO insertion orders: sorted by id
        // and reversed. The pair is the control — insertion order is
        // observable only inside a bucket two ids share, so a tree whose two
        // orders come back EQUAL has no shared bucket and cannot tell a
        // correct port from one that ignores insertion entirely.
        sb.append("  \"hash_orders\": {\n");
        List<AdvancementNode> rootList = new ArrayList<>();
        for (AdvancementNode r : tree.roots()) rootList.add(r);
        rootList.sort(Comparator.comparing(n -> n.holder().id().toString()));
        for (int ri = 0; ri < rootList.size(); ri++) {
            AdvancementNode root = rootList.get(ri);
            List<Identifier> ids = new ArrayList<>();
            collectVisibleCapable(root, ids);
            ids.sort(Comparator.comparing(Identifier::toString));
            sb.append("    \"").append(root.holder().id()).append("\": {\"sorted\": ");
            appendHashSetOrder(sb, ids);
            List<Identifier> rev = new ArrayList<>(ids);
            java.util.Collections.reverse(rev);
            sb.append(", \"reversed\": ");
            appendHashSetOrder(sb, rev);
            sb.append('}');
            if (ri + 1 < rootList.size()) sb.append(',');
            sb.append('\n');
        }
        sb.append("  }\n}\n");
        Files.createDirectories(out.getParent());
        Files.writeString(out, sb.toString(), StandardCharsets.UTF_8);

        log.printf("advancements-golden: %d documents, %d with display, %d granting recipes%n",
                holders.size(), nWithDisplay, nRecipeRewards);
        log.printf("advancements-golden: %d nodes in the tree, %d roots%n",
                tree.nodes().size(), countRoots(tree));
        log.printf("advancements-golden: %d of %d advancements would not encode%n",
                unencodable.size(), holders.size());
        for (String u : unencodable) log.printf("advancements-golden:   unencodable %s%n", u);
    }

    /// Every node under `root`, itself included, whose advancement has a
    /// display — the set `updateTreeVisibility` can put in `added`.
    private static void collectVisibleCapable(AdvancementNode node, List<Identifier> out) {
        if (node.holder().value().display().isPresent()) out.add(node.holder().id());
        for (AdvancementNode c : node.children()) collectVisibleCapable(c, out);
    }

    /// The order a `java.util.HashSet<Identifier>` iterates after those
    /// insertions, taken from the real container rather than computed.
    private static void appendHashSetOrder(StringBuilder sb, List<Identifier> insertion) {
        java.util.Set<Identifier> set = new java.util.HashSet<>();
        set.addAll(insertion);
        sb.append('[');
        boolean f = true;
        for (Identifier id : set) {
            if (!f) sb.append(", ");
            f = false;
            sb.append('"').append(id).append('"');
        }
        sb.append(']');
    }

    private static int countRoots(AdvancementTree tree) {
        int n = 0;
        for (AdvancementNode ignored : tree.roots()) n++;
        return n;
    }

    /// The trigger's registry id, read off the `CriterionTrigger` the parsed
    /// criterion holds. Printed as an id and never as an object: an object's
    /// `toString` is `Object.toString` for most of these and carries a fresh
    /// identity hash per JVM run.
    private static String triggerId(Criterion<?> crit) {
        var trigger = crit.trigger();
        var key = net.minecraft.core.registries.BuiltInRegistries.TRIGGER_TYPES.getKey(trigger);
        if (key == null) {
            throw new IllegalStateException(
                    "trigger not in BuiltInRegistries.TRIGGER_TYPES: "
                            + trigger.getClass().getName());
        }
        return key.toString();
    }

    private static String hex(byte[] b) {
        StringBuilder s = new StringBuilder(b.length * 2);
        for (byte x : b) s.append(String.format("%02x", x));
        return s.toString();
    }
}
