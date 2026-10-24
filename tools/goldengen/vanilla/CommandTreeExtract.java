// CommandTreeExtract — vanilla's WHOLE command graph as semantics, plus the
// bytes vanilla itself puts on the wire for it.
//
// # Why this exists, and how it differs from the two dumpers beside it
//
// `CommandsGolden` encodes the tree *rustmc* registers and gates our wire
// arithmetic against vanilla's encoder. It is a strong gate for everything our
// tree can express, and our tree is three literal nodes, so it is silent about
// argument nodes, redirects, suggestion providers, deep nesting and every
// numbering question a branching tree could ask. Its own header says so.
//
// `CommandTreeGolden` counts the shape — roots, nodes, parsers, executors — and
// answers "how much of the command surface is structure rather than code". It
// counts; it does not let anything be reproduced.
//
// This dumper exists to make the vocabulary claim FALSIFIABLE. The claim is
// that vanilla's 2,000-odd command nodes are a table driven by a small set of
// argument serialisers, and the way to test that is to hand a port the
// semantics and require it to rebuild vanilla's own bytes from them. So this
// writes two things that must be read together:
//
//   * `nodes` — the graph, in SEMANTIC terms. Every node's kind, its name, its
//     parser's registry name, the parser's own properties as vanilla's
//     `ArgumentTypeInfo.serializeToJson` names them, its suggestion provider,
//     whether it is executable, the lowest permission level whose source may
//     use it, its children **in registration order**, and its redirect.
//   * `levels` — the exact `ClientboundCommandsPacket` body vanilla's own
//     stream codec emits for that graph, once per permission level, after
//     vanilla's own `fillUsableCommands` has filtered it.
//
// # The line this file will not cross, and it is the whole point
//
// **No wire encoding appears in `nodes`.** Not a flag byte, not a varint, not a
// node index, not a serialised property blob. `serializeToJson` is used rather
// than `serializeToNetwork` precisely because it emits `{"min": 0}` where the
// network emits `0x01 0x00 0x00 0x00 0x00`, and it OMITS a bound that is the
// type's sentinel, so even the has-min/has-max flag bits have to be re-derived
// by whoever reads this.
//
// A dumper that emitted the property bytes would let a port pass by copying
// them, and the gate would then be measuring its own input — this project's
// central defect, recorded at the top of `docs/AGENT-BRIEF.md`. Everything a
// reader must compute for itself:
//
//   * the flag byte (`1` literal / `2` argument / `4` executable / `8` redirect
//     / `16` custom suggestions / `32` restricted);
//   * the restricted bit, which is NOT dumped — it is a property of the node's
//     requirement against a no-permission source and a reader has to decide it
//     from the level;
//   * the breadth-first node numbering, and every child index derived from it;
//   * `fillUsableCommands`' filter, including that it recurses only into
//     children that survived, and that a redirect whose target has not been
//     copied yet becomes no redirect at all;
//   * every argument type's property encoding, and the varint registry id of
//     its `ArgumentTypeInfo`.
//
// The last of those is data and is dumped: `parsers` maps each registry name to
// the id vanilla assigns it. That is the same kind of thing as an opcode's
// name — it says which serialiser runs, never what the serialiser does.
//
// # The array order is deliberately not the wire order
//
// `nodes` is in depth-first discovery order. The packet numbers nodes
// breadth-first. A reader that used the array order as the node index would
// produce a well-formed packet with every child index wrong, and the byte
// comparison says so on the first entry. `node_order_is_not_wire_order` in the
// Rust test asserts the two orders really do differ, so that this property
// cannot quietly stop holding.
//
// Output: JSON to the file named by `-Drustmc.cmd.out`. Not stdout, because
// `Bootstrap.bootStrap()` re-emits stdout through log4j with a prefix.

package net.minecraft.commands;

import java.io.PrintStream;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Deque;
import java.util.HashMap;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.arguments.ArgumentType;
import com.mojang.brigadier.suggestion.SuggestionProvider;
import com.mojang.brigadier.tree.ArgumentCommandNode;
import com.mojang.brigadier.tree.CommandNode;
import com.mojang.brigadier.tree.LiteralCommandNode;
import com.mojang.brigadier.tree.RootCommandNode;

import io.netty.buffer.Unpooled;

import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.commands.synchronization.ArgumentTypeInfo;
import net.minecraft.commands.synchronization.ArgumentTypeInfos;
import net.minecraft.commands.synchronization.SuggestionProviders;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.protocol.game.ClientboundCommandsPacket;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.server.permissions.PermissionLevel;
import net.minecraft.world.flag.FeatureFlags;

public final class CommandTreeExtract {

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        for (var pending : BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(lookup)) {
            pending.apply();
        }

        CommandBuildContext ctx = CommandBuildContext.simple(lookup, FeatureFlags.VANILLA_SET);
        Commands commands = new Commands(Commands.CommandSelection.DEDICATED, ctx);
        CommandDispatcher<CommandSourceStack> dispatcher = commands.getDispatcher();
        RootCommandNode<CommandSourceStack> root = dispatcher.getRoot();

        JsonObject doc = new JsonObject();
        doc.addProperty("note",
            "vanilla's whole command graph in semantic terms, plus vanilla's own "
            + "ClientboundCommandsPacket bytes for it per permission level. No wire "
            + "encoding appears under `nodes` — see this dumper's header.");
        doc.addProperty("version", SharedConstants.getCurrentVersion().name());

        // --- the parser vocabulary, with the ids vanilla assigns -------------
        //
        // `ClientboundCommandsPacket$ArgumentNodeStub.serializeCap` writes
        // `COMMAND_ARGUMENT_TYPE.getId(info)` as a varint before the properties,
        // so the id is part of the wire format and is not derivable from
        // anything else in this file. It is dumped for the same reason a loot
        // table's opcode name is: it names which serialiser runs and says
        // nothing about what the serialiser writes.
        JsonObject parsers = new JsonObject();
        for (ArgumentTypeInfo<?, ?> info : BuiltInRegistries.COMMAND_ARGUMENT_TYPE) {
            Identifier key = BuiltInRegistries.COMMAND_ARGUMENT_TYPE.getKey(info);
            if (key == null) {
                throw new IllegalStateException("an ArgumentTypeInfo with no registry key");
            }
            parsers.addProperty(key.toString(), BuiltInRegistries.COMMAND_ARGUMENT_TYPE.getId(info));
        }
        doc.add("parsers", parsers);

        // --- the five sources, built exactly as `Commands.sendCommands` does -
        PermissionLevel[] levels = PermissionLevel.values();
        List<CommandSourceStack> sources = new ArrayList<>();
        for (PermissionLevel level : levels) {
            sources.add(Commands.createCompilationContext(LevelBasedPermissionSet.forLevel(level)));
        }

        // --- the graph, depth first ----------------------------------------
        IdentityHashMap<CommandNode<CommandSourceStack>, Integer> ids = new IdentityHashMap<>();
        List<CommandNode<CommandSourceStack>> order = new ArrayList<>();
        discover(root, ids, order);

        JsonArray nodes = new JsonArray();
        int argumentNodes = 0;
        int redirects = 0;
        int executable = 0;
        int suggestionNodes = 0;
        for (CommandNode<CommandSourceStack> node : order) {
            JsonObject n = new JsonObject();
            if (node instanceof RootCommandNode) {
                n.addProperty("kind", "root");
            } else if (node instanceof LiteralCommandNode<CommandSourceStack> lit) {
                n.addProperty("kind", "literal");
                n.addProperty("name", lit.getLiteral());
            } else if (node instanceof ArgumentCommandNode<CommandSourceStack, ?> arg) {
                argumentNodes++;
                n.addProperty("kind", "argument");
                n.addProperty("name", arg.getName());
                ArgumentTypeInfo.Template<?> template = ArgumentTypeInfos.unpack(arg.getType());
                Identifier key = BuiltInRegistries.COMMAND_ARGUMENT_TYPE.getKey(template.type());
                if (key == null) {
                    throw new IllegalStateException(
                        "argument node " + arg.getName() + " has an unregistered type "
                        + arg.getType().getClass().getName());
                }
                n.addProperty("parser", key.toString());
                JsonObject props = new JsonObject();
                serializeCap(props, template.type(), template);
                n.add("properties", props);
                SuggestionProvider<CommandSourceStack> custom = arg.getCustomSuggestions();
                if (custom != null) {
                    suggestionNodes++;
                    n.addProperty("suggestions", SuggestionProviders.getName(custom).toString());
                }
            } else {
                throw new UnsupportedOperationException(
                    "unknown node class " + node.getClass().getName());
            }

            boolean isExecutable = node.getCommand() != null;
            n.addProperty("executable", isExecutable);
            if (isExecutable) {
                executable++;
            }
            n.addProperty("level", levelThreshold(node, sources, levels));

            JsonArray kids = new JsonArray();
            for (CommandNode<CommandSourceStack> k : node.getChildren()) {
                kids.add(ids.get(k).intValue());
            }
            n.add("children", kids);

            if (node.getRedirect() != null) {
                redirects++;
                n.addProperty("redirect", ids.get(node.getRedirect()).intValue());
            }
            nodes.add(n);
        }
        doc.add("nodes", nodes);
        doc.addProperty("root", ids.get(root).intValue());

        // --- vanilla's own bytes, per level ---------------------------------
        Field inspectorField = Commands.class.getDeclaredField("COMMAND_NODE_INSPECTOR");
        inspectorField.setAccessible(true);
        @SuppressWarnings("unchecked")
        ClientboundCommandsPacket.NodeInspector<CommandSourceStack> inspector =
            (ClientboundCommandsPacket.NodeInspector<CommandSourceStack>) inspectorField.get(null);

        Method fill = Commands.class.getDeclaredMethod(
            "fillUsableCommands",
            CommandNode.class, CommandNode.class, Object.class, Map.class);
        fill.setAccessible(true);

        JsonArray levelArray = new JsonArray();
        for (int i = 0; i < levels.length; i++) {
            Map<CommandNode<CommandSourceStack>, CommandNode<CommandSourceStack>> mapping =
                new HashMap<>();
            RootCommandNode<CommandSourceStack> filtered = new RootCommandNode<>();
            mapping.put(root, filtered);
            fill.invoke(null, root, filtered, sources.get(i), mapping);

            ClientboundCommandsPacket packet = new ClientboundCommandsPacket(filtered, inspector);
            FriendlyByteBuf buf = new FriendlyByteBuf(Unpooled.buffer());
            ClientboundCommandsPacket.STREAM_CODEC.encode(buf, packet);
            byte[] bytes = new byte[buf.readableBytes()];
            buf.getBytes(0, bytes);

            JsonObject l = new JsonObject();
            l.addProperty("level", levels[i].name());
            l.addProperty("ordinal", levels[i].ordinal());
            l.addProperty("packetLen", bytes.length);
            l.addProperty("packetHex", hex(bytes));
            levelArray.add(l);
        }
        doc.add("levels", levelArray);

        JsonObject counts = new JsonObject();
        counts.addProperty("roots", root.getChildren().size());
        counts.addProperty("nodes", order.size());
        counts.addProperty("arguments", argumentNodes);
        counts.addProperty("executable", executable);
        counts.addProperty("redirects", redirects);
        counts.addProperty("custom_suggestions", suggestionNodes);
        counts.addProperty("parsers_registered", parsers.size());
        doc.add("counts", counts);

        String dest = System.getProperty("rustmc.cmd.out");
        if (dest == null) {
            throw new IllegalStateException("-Drustmc.cmd.out is required");
        }
        try (PrintStream out = new PrintStream(java.nio.file.Files.newOutputStream(
                java.nio.file.Path.of(dest)), false, StandardCharsets.UTF_8)) {
            out.println(new Gson().toJson(doc));
        }

        Bootstrap.STDOUT.println("command-tree-extract: " + root.getChildren().size()
            + " roots, " + order.size() + " nodes, " + argumentNodes + " argument nodes, "
            + parsers.size() + " parsers registered, " + executable + " executable, "
            + redirects + " redirects, " + suggestionNodes + " custom-suggestion nodes");
    }

    /// Depth-first discovery, following children in registration order and then
    /// the redirect.
    ///
    /// Iterative rather than recursive: `/execute` redirects into itself and
    /// vanilla's tree is deep enough that a naive recursion is a real stack
    /// risk. The explicit stack is pushed in reverse so that the pop order is
    /// the registration order — a detail that does not affect correctness of
    /// the encoding (a reader re-numbers everything) but does keep two runs of
    /// this dumper byte-identical, which `ci/command-tree-extract.sh` checks.
    private static void discover(CommandNode<CommandSourceStack> from,
                                 IdentityHashMap<CommandNode<CommandSourceStack>, Integer> ids,
                                 List<CommandNode<CommandSourceStack>> order) {
        Deque<CommandNode<CommandSourceStack>> stack = new ArrayDeque<>();
        stack.push(from);
        while (!stack.isEmpty()) {
            CommandNode<CommandSourceStack> node = stack.pop();
            if (ids.containsKey(node)) {
                continue;
            }
            ids.put(node, order.size());
            order.add(node);
            List<CommandNode<CommandSourceStack>> kids = new ArrayList<>(node.getChildren());
            if (node.getRedirect() != null) {
                stack.push(node.getRedirect());
            }
            for (int i = kids.size() - 1; i >= 0; i--) {
                stack.push(kids.get(i));
            }
        }
    }

    /// The lowest permission level whose source may use this node, or -1 if no
    /// level may.
    ///
    /// A single threshold is only an honest summary of a Brigadier requirement
    /// if that requirement is monotone in the level, and vanilla's are, because
    /// `Commands.LEVEL_*` are `AlwaysPass` and four `HasCommandLevel` checks.
    /// **That is checked here rather than assumed**: the five booleans are
    /// evaluated and a non-monotone node aborts the dump by name. A dumper that
    /// silently collapsed a non-monotone predicate to a threshold would hand a
    /// port a model of the permission filter that cannot be right, and the
    /// resulting byte mismatch would look like a defect in the port.
    private static int levelThreshold(CommandNode<CommandSourceStack> node,
                                      List<CommandSourceStack> sources,
                                      PermissionLevel[] levels) {
        boolean[] usable = new boolean[sources.size()];
        for (int i = 0; i < sources.size(); i++) {
            usable[i] = node.canUse(sources.get(i));
        }
        for (int i = 1; i < usable.length; i++) {
            if (!usable[i] && usable[i - 1]) {
                StringBuilder seen = new StringBuilder();
                for (int k = 0; k < usable.length; k++) {
                    seen.append(levels[k].name()).append('=').append(usable[k]).append(' ');
                }
                throw new IllegalStateException(
                    "node '" + node.getName() + "' has a non-monotone permission requirement: "
                    + seen + "— a single threshold cannot describe it");
            }
        }
        for (int i = 0; i < usable.length; i++) {
            if (usable[i]) {
                return i;
            }
        }
        return -1;
    }

    /// The generic capture `ArgumentUtils.serializeArgumentCap` performs, which
    /// exists only so `T` is bound when `serializeToJson` is called.
    @SuppressWarnings("unchecked")
    private static <A extends ArgumentType<?>, T extends ArgumentTypeInfo.Template<A>>
    void serializeCap(JsonObject out, ArgumentTypeInfo<A, T> info,
                      ArgumentTypeInfo.Template<?> template) {
        info.serializeToJson((T) template, out);
    }

    private static String hex(byte[] bytes) {
        StringBuilder sb = new StringBuilder(bytes.length * 2);
        for (byte b : bytes) {
            sb.append(Character.forDigit((b >> 4) & 0xF, 16));
            sb.append(Character.forDigit(b & 0xF, 16));
        }
        return sb.toString();
    }

    private CommandTreeExtract() {}
}
