// CommandSurfaceDump — what a *dedicated* vanilla server registers, and the
// declare-commands packet a non-operator receives from it.
//
// WHY THIS EXISTS ALONGSIDE CommandsGolden
//
// `CommandsGolden` builds a dispatcher holding exactly the commands rustmc
// implements and encodes it with vanilla's own codec. That is the right gate
// for "are our bytes vanilla's bytes", and it is structurally blind to the
// only question this file asks: **what else is there**. Its dispatcher holds
// what we chose to put in it, so it can never name a command we have not
// thought of. Asking it for the command surface is asking our own registry
// and getting our own answer back.
//
// So this one never mentions an individual command. It constructs
//
//     new Commands(Commands.CommandSelection.DEDICATED, ctx)
//
// and reports what came out. The selection is the point: the reports
// generator uses `ALL`, which includes the `includeIntegrated` block, and a
// dedicated server has no `/publish`.
//
// WHY IT ALSO ENCODES A PACKET
//
// The root names alone would leave two claims untested. First, that
// `ci/command-surface.py`'s static reading of `Commands.<init>` agrees with
// what the constructor actually does when run — two derivations, one from
// bytecode and one from execution, which can disagree and therefore mean
// something when they do not. Second, that a rustmc trace decoder can read a
// real tree: our own packet has two roots and one branching parent, so a
// decoder tested only against it is a decoder tested against nothing.
//
// The packet is encoded for `PermissionLevel.ALL` because that is what the
// gameplay corpus's players hold. `fillUsableCommands` filters to what that
// source may use, so the encoded root set is a *subset* of the registered
// one, and the gap between them is the permission filter doing its job.
// Both numbers are reported; conflating them would hide the filter.
//
// Same signature-stripping, same reflection into `fillUsableCommands` and
// `COMMAND_NODE_INSPECTOR`, and the same `2>/dev/null` as CommandsGolden:
// both are private, and `Bootstrap.bootStrap()` re-emits stdout through
// log4j.
package net.minecraft.commands;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import com.mojang.brigadier.tree.CommandNode;
import com.mojang.brigadier.tree.RootCommandNode;

import io.netty.buffer.Unpooled;

import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.protocol.game.ClientboundCommandsPacket;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.server.permissions.PermissionLevel;

public final class CommandSurfaceDump {

    @SuppressWarnings("unchecked")
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        HolderLookup.Provider provider = VanillaRegistries.createLookup();
        CommandBuildContext ctx = Commands.createValidationContext(provider);
        Commands commands = new Commands(Commands.CommandSelection.DEDICATED, ctx);

        List<String> registered = new ArrayList<>();
        for (CommandNode<CommandSourceStack> child
                : commands.getDispatcher().getRoot().getChildren()) {
            registered.add(child.getName());
        }

        Field inspectorField = Commands.class.getDeclaredField("COMMAND_NODE_INSPECTOR");
        inspectorField.setAccessible(true);
        ClientboundCommandsPacket.NodeInspector<CommandSourceStack> inspector =
            (ClientboundCommandsPacket.NodeInspector<CommandSourceStack>) inspectorField.get(null);

        Method fill = Commands.class.getDeclaredMethod(
            "fillUsableCommands", CommandNode.class, CommandNode.class, Object.class, Map.class);
        fill.setAccessible(true);

        Map<CommandNode<CommandSourceStack>, CommandNode<CommandSourceStack>> mapping =
            new HashMap<>();
        RootCommandNode<CommandSourceStack> filtered = new RootCommandNode<>();
        mapping.put(commands.getDispatcher().getRoot(), filtered);
        CommandSourceStack source = Commands.createCompilationContext(
            LevelBasedPermissionSet.forLevel(PermissionLevel.ALL));
        fill.invoke(null, commands.getDispatcher().getRoot(), filtered, source, mapping);

        List<String> usable = new ArrayList<>();
        for (CommandNode<CommandSourceStack> child : filtered.getChildren()) {
            usable.add(child.getName());
        }

        FriendlyByteBuf buf = new FriendlyByteBuf(Unpooled.buffer());
        ClientboundCommandsPacket.STREAM_CODEC.encode(
            buf, new ClientboundCommandsPacket(filtered, inspector));
        byte[] bytes = new byte[buf.readableBytes()];
        buf.getBytes(0, bytes);
        StringBuilder hex = new StringBuilder();
        for (byte b : bytes) {
            hex.append(String.format("%02x", b));
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"note\": \"vanilla's own dedicated-server command tree; ")
           .append("registered is every root, usable is what PermissionLevel.ALL sees\",\n");
        out.append("  \"selection\": \"DEDICATED\",\n");
        out.append("  \"registered\": ").append(jsonList(registered)).append(",\n");
        out.append("  \"usableAtAll\": ").append(jsonList(usable)).append(",\n");
        out.append("  \"packetHexAtAll\": \"").append(hex).append("\"\n");
        out.append("}\n");
        Bootstrap.STDOUT.print(out);
    }

    private static String jsonList(List<String> xs) {
        StringBuilder b = new StringBuilder("[");
        for (int i = 0; i < xs.size(); i++) {
            if (i > 0) {
                b.append(", ");
            }
            b.append('"').append(xs.get(i)).append('"');
        }
        return b.append(']').toString();
    }
}
