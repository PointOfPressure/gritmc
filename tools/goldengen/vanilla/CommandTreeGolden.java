// CommandTreeGolden — the SHAPE of every vanilla command, as data.
//
// # What this is for
//
// `docs/ASSESSMENT-2026-08-19.md` extrapolated command roots at **180 weeks**,
// the worst row in the whole census, from a measured rate of zero new roots in a
// fortnight. That extrapolation prices every command as hand-written code.
//
// This dumper tests whether the *shape* of the surface is code at all. Vanilla's
// entire dispatcher builds with no world, no server and no player —
// `new Commands(CommandSelection.DEDICATED, CommandBuildContext.simple(lookup,
// FeatureFlags.VANILLA_SET))` — and Brigadier's tree is then walkable: every
// literal, every argument node with its parser id, every redirect, and whether
// each node carries an executor.
//
// # The line this dumper draws, and it is the important one
//
// A command node is two separable things:
//
//   * its **shape** — the literals, the argument names and parser types, the
//     redirects, the child ordering, which nodes are executable. All of that is
//     structure, and this file extracts it.
//   * its **executor** — the lambda that runs when the node is reached. That is
//     bytecode, it is where the behaviour lives, and **nothing here can see
//     inside it**. `executable: true` says a lambda exists, not what it does.
//
// So the count of nodes is the size of the extractable half, and the count of
// executors is the size of the half that must be written. Reporting the first
// without the second would be the `use_item_on` `result`-column mistake again:
// a tree with every node in the right place and no executors is a server that
// autocompletes perfectly and does nothing.
//
// Output: JSON to the file named by `-Drustmc.cmd.out`. Not stdout — see
// `LootGolden`'s header for why.

package net.minecraft.commands;

import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

import com.mojang.brigadier.tree.ArgumentCommandNode;
import com.mojang.brigadier.tree.CommandNode;
import com.mojang.brigadier.tree.LiteralCommandNode;
import com.mojang.brigadier.tree.RootCommandNode;

import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.flag.FeatureFlags;

public final class CommandTreeGolden {

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        for (var pending : BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(lookup)) {
            pending.apply();
        }

        CommandBuildContext ctx = CommandBuildContext.simple(lookup, FeatureFlags.VANILLA_SET);
        Commands commands = new Commands(Commands.CommandSelection.DEDICATED, ctx);
        RootCommandNode<CommandSourceStack> root = commands.getDispatcher().getRoot();

        String dest = System.getProperty("rustmc.cmd.out");
        if (dest == null) throw new IllegalStateException("-Drustmc.cmd.out is required");
        PrintStream out = new PrintStream(java.nio.file.Files.newOutputStream(
                java.nio.file.Path.of(dest)), false, StandardCharsets.UTF_8);

        Counts c = new Counts();
        StringBuilder body = new StringBuilder();
        List<String> roots = new ArrayList<>();
        for (CommandNode<CommandSourceStack> child : root.getChildren()) {
            roots.add(child.getName());
        }
        java.util.Collections.sort(roots);

        body.append("  \"roots\": [");
        for (int i = 0; i < roots.size(); i++) {
            if (i > 0) body.append(", ");
            body.append("\"").append(esc(roots.get(i))).append("\"");
        }
        body.append("],\n");

        body.append("  \"tree\": {\n");
        boolean first = true;
        for (String name : roots) {
            CommandNode<CommandSourceStack> child = root.getChild(name);
            if (!first) body.append(",\n");
            first = false;
            body.append("    \"").append(esc(name)).append("\": ");
            walk(child, body, c, new IdentityHashMap<>(), 0);
        }
        body.append("\n  },\n");

        out.println("{");
        out.print(body);
        out.println("  \"roots_total\": " + roots.size() + ",");
        out.println("  \"nodes_total\": " + c.nodes + ",");
        out.println("  \"literals\": " + c.literals + ",");
        out.println("  \"arguments\": " + c.arguments + ",");
        out.println("  \"executable_nodes\": " + c.executable + ",");
        out.println("  \"redirects\": " + c.redirects + ",");
        out.print("  \"argument_parsers\": {");
        boolean f2 = true;
        for (Map.Entry<String, Integer> e : c.parsers.entrySet()) {
            if (!f2) out.print(", ");
            f2 = false;
            out.print("\"" + esc(e.getKey()) + "\": " + e.getValue());
        }
        out.println("},");
        out.println("  \"distinct_argument_parsers\": " + c.parsers.size());
        out.println("}");
        out.flush();
        out.close();

        Bootstrap.STDOUT.println("command-tree: " + roots.size() + " roots, " + c.nodes
                + " nodes, " + c.arguments + " argument nodes, " + c.parsers.size()
                + " distinct parsers, " + c.executable + " executable nodes, "
                + c.redirects + " redirects");
    }

    private static final class Counts {
        int nodes, literals, arguments, executable, redirects;
        final Map<String, Integer> parsers = new TreeMap<>();
    }

    /// Walks one subtree. `seen` guards against the cycles redirects create —
    /// `/execute` redirects back to itself, so a naive recursion never returns.
    private static void walk(CommandNode<CommandSourceStack> node, StringBuilder sb, Counts c,
                             IdentityHashMap<CommandNode<?>, Boolean> seen, int depth) {
        c.nodes++;
        boolean cycle = seen.put(node, Boolean.TRUE) != null;
        sb.append("{\"kind\": \"");
        if (node instanceof LiteralCommandNode) {
            c.literals++;
            sb.append("literal");
        } else if (node instanceof ArgumentCommandNode<?, ?> a) {
            c.arguments++;
            sb.append("argument");
            String parser = a.getType().getClass().getName();
            c.parsers.merge(parser, 1, Integer::sum);
            sb.append("\", \"parser\": \"").append(esc(parser));
        } else {
            sb.append("root");
        }
        sb.append("\", \"executable\": ").append(node.getCommand() != null);
        if (node.getCommand() != null) c.executable++;
        if (node.getRedirect() != null) {
            c.redirects++;
            sb.append(", \"redirect\": \"").append(esc(node.getRedirect().getName())).append("\"");
        }
        if (cycle || depth > 24) {
            sb.append(", \"truncated\": true}");
            return;
        }
        var kids = node.getChildren();
        if (!kids.isEmpty()) {
            sb.append(", \"children\": {");
            boolean first = true;
            List<CommandNode<CommandSourceStack>> sorted = new ArrayList<>(kids);
            sorted.sort(java.util.Comparator.comparing(CommandNode::getName));
            for (CommandNode<CommandSourceStack> k : sorted) {
                if (!first) sb.append(", ");
                first = false;
                sb.append("\"").append(esc(k.getName())).append("\": ");
                walk(k, sb, c, seen, depth + 1);
            }
            sb.append("}");
        }
        sb.append("}");
        seen.remove(node);
    }

    private static String esc(String s) {
        return s.replace("\\", "\\\\").replace("\"", "\\\"");
    }

    private CommandTreeGolden() {}
}
