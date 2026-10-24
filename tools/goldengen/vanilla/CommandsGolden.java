// CommandsGolden — vanilla's OWN bytes for the declare-commands packet, for a
// dispatcher holding exactly the commands this server implements.
//
// WHY THIS SHAPE, AND NOT A CAPTURE FROM A LIVE SERVER
//
// The obvious gate for `ClientboundCommandsPacket` is to boot vanilla, record
// the packet, and require ours to match. That gate cannot exist: vanilla's
// dispatcher holds every command in the game and ours holds three nodes, so
// the two packets differ in the only thing a byte comparison can see. Node
// indices are assigned by a breadth-first walk over the whole tree
// (`enumerateNodes`), so *every* index in vanilla's packet depends on
// commands we do not implement, and even the entry for `list` is not a
// substring of vanilla's bytes.
//
// A capture also cannot be parsed our way round: to reach entry k you must
// walk entries 0..k-1, and vanilla's tree is full of argument nodes whose
// payload is written by `ArgumentTypeInfo.Template` — about fifty distinct
// serialisers. Skipping one requires implementing it.
//
// So this dumper builds a dispatcher containing **only** the commands rustmc
// registers, using vanilla's own `register` methods, and then encodes it with
// vanilla's own `ClientboundCommandsPacket` constructor and stream codec. The
// bytes that come out are what vanilla would put on the wire for our tree.
// That is a real gate against vanilla's behaviour rather than a second
// reading of its bytecode: if our flag arithmetic, our BFS numbering, our
// child ordering or our string encoding is wrong by one bit, these bytes say
// so.
//
// Every node in our tree is a literal, which is why no argument-type
// serialiser is needed here. **If a command with an argument is ever added,
// this dumper keeps working and the Rust encoder does not** — see
// `crate::commandtree`'s header for what it refuses.
//
// THE PERMISSION HALF
//
// `Commands.sendCommands` does not send the whole tree. It first runs
// `fillUsableCommands`, which copies across only the nodes whose Brigadier
// requirement accepts *that player's* source, and the packet is built from
// the copy. Separately, `Commands.COMMAND_NODE_INSPECTOR.isRestricted` sets
// bit 32 on any node whose requirement rejects a `NO_PERMISSIONS` source.
//
// Both are vanilla behaviour that a port gets wrong silently — a server that
// skips the filter sends `/seed` to everyone and the client offers a
// completion for a command it will be refused — so the dump is taken at
// every permission level, and `/seed` appearing and disappearing across them
// is the thing the Rust test asserts. A dump at one level would be a gate
// that cannot fail.
//
// `fillUsableCommands` and `COMMAND_NODE_INSPECTOR` are both private, so both
// are reached by reflection. That is deliberate: the alternative is
// reimplementing them here, and a golden that reimplements the thing it is
// gating is not a golden.

package net.minecraft.commands;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.arguments.ArgumentType;
import com.mojang.brigadier.arguments.BoolArgumentType;
import com.mojang.brigadier.arguments.IntegerArgumentType;
import com.mojang.brigadier.builder.LiteralArgumentBuilder;
import com.mojang.brigadier.tree.CommandNode;
import com.mojang.brigadier.tree.RootCommandNode;

import io.netty.buffer.Unpooled;

import net.minecraft.SharedConstants;
import net.minecraft.core.UUIDUtil;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.ComponentSerialization;
import net.minecraft.network.chat.ComponentUtils;
import net.minecraft.network.protocol.game.ClientboundCommandsPacket;
import net.minecraft.server.Bootstrap;
import net.minecraft.commands.CommandSource;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import com.mojang.brigadier.arguments.StringArgumentType;
import net.minecraft.world.scores.Scoreboard;
import net.minecraft.world.scores.Objective;
import net.minecraft.world.scores.DisplaySlot;
import net.minecraft.world.scores.criteria.ObjectiveCriteria;
import net.minecraft.commands.arguments.ObjectiveArgument;
import net.minecraft.commands.arguments.ObjectiveCriteriaArgument;
import net.minecraft.commands.arguments.ScoreboardSlotArgument;
import net.minecraft.ChatFormatting;
import net.minecraft.world.Difficulty;
import net.minecraft.server.commands.DifficultyCommand;
import net.minecraft.server.commands.GameModeCommand;
import net.minecraft.server.commands.TeleportCommand;
import net.minecraft.server.commands.TitleCommand;
import net.minecraft.server.commands.SetBlockCommand;
import net.minecraft.commands.CommandBuildContext;
import net.minecraft.core.HolderLookup;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.server.commands.TimeCommand;
import net.minecraft.server.commands.WeatherCommand;
import net.minecraft.server.commands.ExperienceCommand;
import net.minecraft.server.commands.DefaultGameModeCommands;
import net.minecraft.commands.arguments.EntityArgument;
import net.minecraft.commands.arguments.coordinates.Vec3Argument;
import net.minecraft.world.level.GameType;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import com.mojang.brigadier.exceptions.SimpleCommandExceptionType;
import com.mojang.brigadier.exceptions.DynamicCommandExceptionType;
import com.mojang.brigadier.exceptions.Dynamic2CommandExceptionType;
import net.minecraft.server.commands.EmoteCommands;
import net.minecraft.server.commands.HelpCommand;
import net.minecraft.server.commands.ListPlayersCommand;
import net.minecraft.server.commands.MsgCommand;
import net.minecraft.server.commands.SayCommand;
import net.minecraft.server.commands.SeedCommand;
import net.minecraft.server.commands.WorldBorderCommand;
import net.minecraft.server.commands.VersionCommand;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.server.permissions.PermissionLevel;

public final class CommandsGolden {

    /// The commands rustmc registers, registered here in the same order.
    ///
    /// Order is load-bearing and is not a style choice: Brigadier's
    /// `CommandNode.children` is a `LinkedHashMap`, so `getChildren()` yields
    /// insertion order, and `enumerateNodes` walks that order breadth-first
    /// to assign the indices every entry's child array refers to. Registering
    /// `seed` before `list` produces a different, equally valid packet — and
    /// a Rust port that registered them the other way would fail this golden,
    /// which is exactly what it is for.
    private static void registerOurs(CommandDispatcher<CommandSourceStack> d) {
        ListPlayersCommand.register(d);
        // `true` is `CommandSelection.DEDICATED.includeDedicated`, which is
        // what a dedicated server passes. It is the argument that makes
        // `/seed` require GAMEMASTERS instead of ALL, so passing `false` here
        // would silently produce a golden in which the permission filter has
        // nothing to filter.
        SeedCommand.register(d, true);
        registerGameRules(d);
        // `/version` IS registered now, and the paragraph this replaces is
        // worth keeping in view rather than deleting: it read "deliberately
        // **not** registered ... rustmc must not, until it can answer it. A
        // client offered a command that does nothing is worse on the wire than
        // a client not offered it."
        //
        // That condition is met. `crate::commands::VERSION_LINES` answers it
        // with the ten `system_chat` bodies vanilla sends, nine of them the
        // very `versionLines` this file already dumped and the tenth
        // `commands.version.header`, which `VersionCommand`'s executor sends
        // BEFORE calling `dumpVersion` and which therefore never appeared in
        // that dump. The same `true` as `SeedCommand`: it is
        // `CommandSelection.DEDICATED.includeDedicated` and it is what makes
        // `/version` GAMEMASTERS rather than ALL.
        VersionCommand.register(d, true);
        // `/say` and `/me`. Registration ORDER is wire-visible — Brigadier's
        // `children` is a `LinkedHashMap` and `enumerateNodes` numbers in that
        // order — so these two must sit here, after `/version`, exactly as
        // `crate::commands::build_tree` puts them.
        SayCommand.register(d);
        EmoteCommands.register(d);
        // `MsgCommand.register` registers THREE roots in one call — `msg`,
        // then `tell` and `w` as `redirect(node)` literals pointing at it —
        // and this is the first redirect in our own tree. The redirect edge
        // carries a node index, so getting the registration order wrong here
        // moves that index and the byte comparison says so.
        MsgCommand.register(d);
        // `/help`. Registering it makes its own usage line part of the answer,
        // which is the point: `/help` enumerates the tree it is IN.
        HelpCommand.register(d);
        // `/worldborder`. Appended LAST, and that position is the whole
        // reason this comment exists: `enumerateNodes` is breadth-first over
        // `LinkedHashMap` insertion order, so a root inserted anywhere but the
        // end renumbers every node after it — including the redirect edge
        // `tell` and `w` carry into `msg`, which is a node INDEX on the wire.
        // `crate::commands::build_tree` appends it in the same place.
        WorldBorderCommand.register(d);
        registerScoreboard(d);
        // From here on, the roots `crate::cmdexec` executes, appended in the
        // order `crate::commands::build_tree` appends them. `/difficulty` is
        // all literals, so vanilla's own `register` builds exactly our tree.
        DifficultyCommand.register(d);
        GameModeCommand.register(d);
        registerTeleport(d);
        WeatherCommand.register(d);
        ExperienceCommand.register(d);
        DefaultGameModeCommands.register(d);
        TitleCommand.register(d, context());
        registerTime(d);
        registerExecute(d);
        SetBlockCommand.register(d, context());
        registerFill(d);
        net.minecraft.server.commands.PlaySoundCommand.register(d);
        net.minecraft.server.commands.StopSoundCommand.register(d);
        registerClone(d);
        net.minecraft.server.commands.TellRawCommand.register(d, context());
        registerRandom(d);
        registerRotate(d);
        net.minecraft.server.commands.SwingCommand.register(d);
        net.minecraft.server.commands.ParticleCommand.register(d, context());
        net.minecraft.server.commands.TeamMsgCommand.register(d);
        registerDialog(d);
        net.minecraft.server.commands.BossBarCommands.register(d, context());
    }

    /// `/dialog`, the SUBSET `crate::commands::dialog_root` registers:
    /// `clear <targets>`, without `show`.
    private static void registerDialog(CommandDispatcher<CommandSourceStack> d) {
        d.register(Commands.literal("dialog")
            .requires(Commands.hasPermission(Commands.LEVEL_GAMEMASTERS))
            .then(Commands.literal("clear")
                .then(Commands.argument("targets", EntityArgument.players())
                    .executes(c -> 91))));
    }

    /// `/rotate`, the SUBSET `crate::commands::rotate_root` registers:
    /// `<target> <rotation>`, without the two `facing` branches.
    private static void registerRotate(CommandDispatcher<CommandSourceStack> d) {
        d.register(Commands.literal("rotate")
            .requires(Commands.hasPermission(Commands.LEVEL_GAMEMASTERS))
            .then(Commands.argument("target", net.minecraft.commands.arguments.EntityArgument.entity())
                .then(Commands.argument("rotation",
                        net.minecraft.commands.arguments.coordinates.RotationArgument.rotation())
                    .executes(c -> 81))));
    }

    /// `/random`, the level-0 SUBSET `crate::commands::random_root` registers:
    /// `value <range>` and `roll <range>`, without the level-2 `sequence`
    /// children and `reset`.
    private static void registerRandom(CommandDispatcher<CommandSourceStack> d) {
        d.register(Commands.literal("random")
            .then(Commands.literal("value")
                .then(Commands.argument("range", net.minecraft.commands.arguments.RangeArgument.intRange())
                    .executes(c -> 71)))
            .then(Commands.literal("roll")
                .then(Commands.argument("range", net.minecraft.commands.arguments.RangeArgument.intRange())
                    .executes(c -> 72))));
    }

    /// `/clone`, the SUBSET `crate::commands::clone_root` registers: the
    /// same-level forms without `filtered` and without `from`/`to`.
    private static void registerClone(CommandDispatcher<CommandSourceStack> d) {
        d.register(Commands.literal("clone")
            .requires(Commands.hasPermission(Commands.LEVEL_GAMEMASTERS))
            .then(Commands.argument("begin", net.minecraft.commands.arguments.coordinates.BlockPosArgument.blockPos())
                .then(Commands.argument("end", net.minecraft.commands.arguments.coordinates.BlockPosArgument.blockPos())
                    .then(cloneMasks(Commands.argument("destination",
                            net.minecraft.commands.arguments.coordinates.BlockPosArgument.blockPos()).executes(c -> 51), 52)
                        .then(cloneMasks(Commands.literal("strict").executes(c -> 61), 62))))));
    }

    /// `replace` and `masked`, each executable and each with the three modes.
    /// Distinct return values per node, so `enumerateNodes` cannot merge two
    /// nodes whose `Command` objects compare equal.
    private static <T extends com.mojang.brigadier.builder.ArgumentBuilder<CommandSourceStack, T>> T cloneMasks(T node, int base) {
        for (String mask : List.of("replace", "masked")) {
            final int m = base * 10 + (mask.equals("replace") ? 1 : 5);
            node.then(Commands.literal(mask).executes(c -> m)
                .then(Commands.literal("force").executes(c -> m + 1))
                .then(Commands.literal("move").executes(c -> m + 2))
                .then(Commands.literal("normal").executes(c -> m + 3)));
        }
        return node;
    }

    /// `/fill`, the SUBSET `crate::commands::fill_root` registers: every mode
    /// but `replace <filter>` (a `BlockPredicateArgument` this server does not
    /// parse), with vanilla's own `BlockPosArgument` and `BlockStateArgument`.
    private static void registerFill(CommandDispatcher<CommandSourceStack> d) {
        var block = Commands.argument("block",
                net.minecraft.commands.arguments.blocks.BlockStateArgument.block(context()))
            .executes(c -> 41)
            .then(Commands.literal("outline").executes(c -> 42))
            .then(Commands.literal("hollow").executes(c -> 43))
            .then(Commands.literal("destroy").executes(c -> 44))
            .then(Commands.literal("strict").executes(c -> 45))
            .then(Commands.literal("replace").executes(c -> 46))
            .then(Commands.literal("keep").executes(c -> 47));
        d.register(Commands.literal("fill")
            .requires(Commands.hasPermission(Commands.LEVEL_GAMEMASTERS))
            .then(Commands.argument("from", net.minecraft.commands.arguments.coordinates.BlockPosArgument.blockPos())
                .then(Commands.argument("to", net.minecraft.commands.arguments.coordinates.BlockPosArgument.blockPos())
                    .then(block))));
    }

    private static CommandBuildContext buildContext;

    /// The build context `/setblock`'s `BlockStateArgument` is constructed
    /// with — the same one `CommandParseGolden` and `CommandTreeExtract` use:
    /// no world, no server, vanilla's feature set. The argument serialises no
    /// properties, so the context reaches no byte of the packet; it is needed
    /// only because `register` takes one.
    private static CommandBuildContext context() {
        if (buildContext == null) {
            HolderLookup.Provider lookup = VanillaRegistries.createLookup();
            buildContext = CommandBuildContext.simple(lookup, FeatureFlags.VANILLA_SET);
        }
        return buildContext;
    }

    /// `/execute`, the SUBSET `crate::commands::execute_root` registers, built
    /// the way `ExecuteCommand.register` builds it: the bare literal is
    /// registered FIRST so that the `fork`/`redirect` targets can point at
    /// it, and the second registration merges the children in.
    private static void registerExecute(CommandDispatcher<CommandSourceStack> d) {
        com.mojang.brigadier.tree.LiteralCommandNode<CommandSourceStack> exec = d.register(
            Commands.literal("execute").requires(Commands.hasPermission(Commands.LEVEL_GAMEMASTERS)));
        d.register(Commands.literal("execute")
            .requires(Commands.hasPermission(Commands.LEVEL_GAMEMASTERS))
            .then(Commands.literal("run").redirect(d.getRoot()))
            .then(Commands.literal("as")
                .then(Commands.argument("targets", EntityArgument.entities()).fork(exec, c -> List.of())))
            .then(Commands.literal("at")
                .then(Commands.argument("targets", EntityArgument.entities()).fork(exec, c -> List.of())))
            .then(Commands.literal("positioned")
                .then(Commands.argument("pos", Vec3Argument.vec3()).redirect(exec, c -> c.getSource()))
                .then(Commands.literal("as")
                    .then(Commands.argument("targets", EntityArgument.entities()).fork(exec, c -> List.of())))));
    }

    /// `/time`, the default-clock SUBSET `crate::commands::time_root`
    /// registers: `set <time>`, `add <time>`, `query time`, `query gametime`,
    /// with vanilla's own `TimeArgument` bounds (0 and `Integer.MIN_VALUE`).
    private static void registerTime(CommandDispatcher<CommandSourceStack> d) {
        d.register(Commands.literal("time")
            .requires(Commands.hasPermission(Commands.LEVEL_GAMEMASTERS))
            .then(Commands.literal("set")
                .then(Commands.argument("time", net.minecraft.commands.arguments.TimeArgument.time())
                    .executes(c -> 31)))
            .then(Commands.literal("add")
                .then(Commands.argument("time", net.minecraft.commands.arguments.TimeArgument.time(Integer.MIN_VALUE))
                    .executes(c -> 32)))
            .then(Commands.literal("query")
                .then(Commands.literal("time").executes(c -> 33))
                .then(Commands.literal("gametime").executes(c -> 34))));
    }

    /// `/title`, through vanilla's builders in the SUBSET shape
    /// `crate::commands::title_root` registers: `clear`, `reset` and `times`,
    /// without `title`/`subtitle`/`actionbar`, whose `ComponentArgument` this
    /// server does not parse.
    private static void registerTitle(CommandDispatcher<CommandSourceStack> d) {
        d.register(Commands.literal("title")
            .requires(Commands.hasPermission(Commands.LEVEL_GAMEMASTERS))
            .then(Commands.argument("targets", EntityArgument.players())
                .then(Commands.literal("clear").executes(c -> 21))
                .then(Commands.literal("reset").executes(c -> 22))
                .then(Commands.literal("times")
                    .then(Commands.argument("fadeIn", net.minecraft.commands.arguments.TimeArgument.time())
                        .then(Commands.argument("stay", net.minecraft.commands.arguments.TimeArgument.time())
                            .then(Commands.argument("fadeOut", net.minecraft.commands.arguments.TimeArgument.time())
                                .executes(c -> 23)))))));
    }

    /// `/teleport` and `/tp`, through vanilla's own builders in the SUBSET
    /// shape `crate::commands::teleport_root` registers: everything but
    /// `targets location facing ...`, whose `LookAt` sends a packet this
    /// server has no writer for. `tp` redirects into the `teleport` node exactly as
    /// `TeleportCommand.register` makes it, permission included.
    private static void registerTeleport(CommandDispatcher<CommandSourceStack> d) {
        com.mojang.brigadier.tree.LiteralCommandNode<CommandSourceStack> node = d.register(
            Commands.literal("teleport")
                .requires(Commands.hasPermission(Commands.LEVEL_GAMEMASTERS))
                .then(Commands.argument("location", Vec3Argument.vec3()).executes(c -> 11))
                .then(Commands.argument("destination", EntityArgument.entity()).executes(c -> 12))
                .then(Commands.argument("targets", EntityArgument.entities())
                    .then(Commands.argument("location", Vec3Argument.vec3()).executes(c -> 13)
                        .then(Commands.argument("rotation",
                                net.minecraft.commands.arguments.coordinates.RotationArgument.rotation())
                            .executes(c -> 15)))
                    .then(Commands.argument("destination", EntityArgument.entity()).executes(c -> 14))));
        d.register(Commands.literal("tp")
            .requires(Commands.hasPermission(Commands.LEVEL_GAMEMASTERS))
            .redirect(node));
    }

    /// `/scoreboard`, rebuilt through vanilla's own builders in the SUBSET
    /// shape `crate::commands::scoreboard_root` registers.
    ///
    /// # Why not `ScoreboardCommand.register(d)`
    ///
    /// The same reason `registerGameRules` does not call
    /// `GameRuleCommand.register`: this golden is "vanilla's bytes for OUR
    /// command set", and our set is a subset here. `ScoreboardCommand`
    /// registers `objectives modify`, every `players` branch, the number
    /// formats and the render types — all of which take
    /// `minecraft:component`, `minecraft:score_holder` or a number-format
    /// argument this server cannot read. Registering them would put nodes in
    /// the packet that no executor answers, which is the one thing this
    /// file's own header says is worse than not registering a command at all.
    ///
    /// Every node here is vanilla's own `ArgumentType` instance, so the
    /// property bytes and the parser identifiers are vanilla's even though
    /// the shape is ours. That is the whole point of building it this way
    /// rather than hand-writing the packet.
    ///
    /// The lambdas capture a distinct `int` for the reason `registerGameRules`
    /// spells out: `CommandNode.equals` compares the `Command` object, and a
    /// non-capturing `c -> 0` would make `enumerateNodes` deduplicate the four
    /// executable nodes into one.
    private static void registerScoreboard(CommandDispatcher<CommandSourceStack> d) {
        LiteralArgumentBuilder<CommandSourceStack> root =
            Commands.literal("scoreboard")
                    .requires(Commands.hasPermission(Commands.LEVEL_GAMEMASTERS));
        root.then(Commands.literal("objectives")
            .then(Commands.literal("list").executes(c -> 1))
            // `add` takes `StringArgumentType.word()` and not
            // `ObjectiveArgument`: the objective does not exist yet, and
            // `ObjectiveArgument`'s suggestion provider lists the ones that
            // do. The argument NAME is `objective` in both, and a name is
            // wire-visible, so the pair is not interchangeable.
            .then(Commands.literal("add")
                .then(Commands.argument("objective", StringArgumentType.word())
                    .then(Commands.argument("criteria", ObjectiveCriteriaArgument.criteria())
                        .executes(c -> 2))))
            .then(Commands.literal("remove")
                .then(Commands.argument("objective", ObjectiveArgument.objective())
                    .executes(c -> 3)))
            .then(Commands.literal("setdisplay")
                .then(Commands.argument("slot", ScoreboardSlotArgument.displaySlot())
                    .executes(c -> 4)
                    .then(Commands.argument("objective", ObjectiveArgument.objective())
                        .executes(c -> 5)))));
        d.register(root);
    }


    /// `/gamerule`, rebuilt through vanilla's own builders in **rustmc's**
    /// order rather than by calling `GameRuleCommand.register`.
    ///
    /// # Why not just call vanilla's registration
    ///
    /// Because vanilla's own order is not stable. `GameRuleCommand.register`
    /// walks `new GameRules(ctx.enabledFeatures())` with
    /// `visitGameRuleTypes`, and that iteration reorders between JVM runs:
    /// measured 2026-09-05 over sixteen runs of `CommandTreeExtract` against
    /// this jar, fifteen produced one order and one another, 160 of 2,085
    /// nodes permuted and every one of them a `/gamerule` child. A golden
    /// built from it would fail about one run in sixteen, for no defect.
    /// `ci/command-tree-compare.py` carries the measurement.
    ///
    /// So the ORDER is rustmc's, taken from `crate::gamerules::DEFAULTS`
    /// (the jar's `<clinit>` order, which is stable), and everything else —
    /// the two registered names per rule, the argument types, the declared
    /// bounds, the permission level, the property serialisers and every byte
    /// of the encoding — is vanilla's own. A wrong bound or a wrong type in
    /// `commands::GAME_RULES` still fails this golden; only the order cannot
    /// be gated, because vanilla has no stable answer to gate against.
    ///
    /// `crates/rustmc-server/tests/command_tree_vanilla.rs` closes the other
    /// half order-insensitively: our 58 rules, their two names and their
    /// types and bounds against vanilla's own `/gamerule` subtree in the
    /// extract.
    ///
    /// # The lambdas must capture, and that is not a style point
    ///
    /// `enumerateNodes` deduplicates by Brigadier's structural equality, and
    /// `CommandNode.equals` compares the children map and the `Command`
    /// object. A NON-capturing `c -> 0` is one cached instance for every
    /// call site, so all 94 boolean `value` nodes would be `equals` and the
    /// packet would carry ONE of them. Vanilla's own are
    /// `ctx -> setRule(ctx, rule)`, which capture the rule and are therefore
    /// distinct. `seq++` below is what makes these distinct too.
    private static void registerGameRules(CommandDispatcher<CommandSourceStack> d) {
        LiteralArgumentBuilder<CommandSourceStack> root =
            Commands.literal("gamerule")
                    .requires(Commands.hasPermission(Commands.LEVEL_GAMEMASTERS));
        int seq = 0;
        for (Object[] rule : GAME_RULES) {
            String name = (String) rule[0];
            boolean isBool = "b".equals(rule[1]);
            for (String literal : new String[] { name, "minecraft:" + name }) {
                ArgumentType<?> type = isBool
                    ? BoolArgumentType.bool()
                    : IntegerArgumentType.integer((Integer) rule[2], (Integer) rule[3]);
                final int query = seq++;
                final int set = seq++;
                root.then(Commands.literal(literal)
                          .executes(c -> query)
                          .then(Commands.argument("value", type).executes(c -> set)));
            }
        }
        d.register(root);
    }

    /// The 58 rules `/gamerule` offers, in `crate::gamerules::DEFAULTS`'
    /// order, with the argument type and the bounds vanilla declares.
    ///
    /// `max_minecart_speed` is in the game-rule registry and is **not** here:
    /// `new GameRules(enabledFeatures)` filters by feature flag and it is not
    /// in `FeatureFlags.VANILLA_SET`, so a vanilla dedicated server has the
    /// rule and does not offer the command.
    ///
    /// `Integer.MIN_VALUE` / `Integer.MAX_VALUE` are the sentinels
    /// `IntegerArgumentInfo.serializeToNetwork` compares against, so a bound
    /// written as one is a bound the wire omits — which is exactly the
    /// distinction the Rust side carries as `Option::None`.
    private static final Object[][] GAME_RULES = {
        { "advance_time", "b", 0, 0 },
        { "advance_weather", "b", 0, 0 },
        { "allow_entering_nether_using_portals", "b", 0, 0 },
        { "block_drops", "b", 0, 0 },
        { "block_explosion_drop_decay", "b", 0, 0 },
        { "command_blocks_work", "b", 0, 0 },
        { "command_block_output", "b", 0, 0 },
        { "drowning_damage", "b", 0, 0 },
        { "elytra_movement_check", "b", 0, 0 },
        { "ender_pearls_vanish_on_death", "b", 0, 0 },
        { "entity_drops", "b", 0, 0 },
        { "fall_damage", "b", 0, 0 },
        { "fire_damage", "b", 0, 0 },
        { "fire_spread_radius_around_player", "i", -1, Integer.MAX_VALUE },
        { "forgive_dead_players", "b", 0, 0 },
        { "freeze_damage", "b", 0, 0 },
        { "global_sound_events", "b", 0, 0 },
        { "immediate_respawn", "b", 0, 0 },
        { "keep_inventory", "b", 0, 0 },
        { "lava_source_conversion", "b", 0, 0 },
        { "limited_crafting", "b", 0, 0 },
        { "locator_bar", "b", 0, 0 },
        { "log_admin_commands", "b", 0, 0 },
        { "max_block_modifications", "i", 1, Integer.MAX_VALUE },
        { "max_command_forks", "i", 0, Integer.MAX_VALUE },
        { "max_command_sequence_length", "i", 0, Integer.MAX_VALUE },
        { "max_entity_cramming", "i", 0, Integer.MAX_VALUE },
        { "max_snow_accumulation_height", "i", 0, 8 },
        { "mob_drops", "b", 0, 0 },
        { "mob_explosion_drop_decay", "b", 0, 0 },
        { "mob_griefing", "b", 0, 0 },
        { "natural_health_regeneration", "b", 0, 0 },
        { "player_movement_check", "b", 0, 0 },
        { "players_nether_portal_creative_delay", "i", 0, Integer.MAX_VALUE },
        { "players_nether_portal_default_delay", "i", 0, Integer.MAX_VALUE },
        { "players_sleeping_percentage", "i", 0, Integer.MAX_VALUE },
        { "projectiles_can_break_blocks", "b", 0, 0 },
        { "pvp", "b", 0, 0 },
        { "raids", "b", 0, 0 },
        { "random_tick_speed", "i", 0, Integer.MAX_VALUE },
        { "reduced_debug_info", "b", 0, 0 },
        { "respawn_radius", "i", 0, Integer.MAX_VALUE },
        { "send_command_feedback", "b", 0, 0 },
        { "show_advancement_messages", "b", 0, 0 },
        { "show_death_messages", "b", 0, 0 },
        { "spawner_blocks_work", "b", 0, 0 },
        { "spawn_mobs", "b", 0, 0 },
        { "spawn_monsters", "b", 0, 0 },
        { "spawn_patrols", "b", 0, 0 },
        { "spawn_phantoms", "b", 0, 0 },
        { "spawn_wandering_traders", "b", 0, 0 },
        { "spawn_wardens", "b", 0, 0 },
        { "spectators_generate_chunks", "b", 0, 0 },
        { "spread_vines", "b", 0, 0 },
        { "tnt_explodes", "b", 0, 0 },
        { "tnt_explosion_drop_decay", "b", 0, 0 },
        { "universal_anger", "b", 0, 0 },
        { "water_source_conversion", "b", 0, 0 },
    };

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        Field inspectorField = Commands.class.getDeclaredField("COMMAND_NODE_INSPECTOR");
        inspectorField.setAccessible(true);
        @SuppressWarnings("unchecked")
        ClientboundCommandsPacket.NodeInspector<CommandSourceStack> inspector =
            (ClientboundCommandsPacket.NodeInspector<CommandSourceStack>) inspectorField.get(null);

        Method fill = Commands.class.getDeclaredMethod(
            "fillUsableCommands",
            CommandNode.class, CommandNode.class, Object.class, Map.class);
        fill.setAccessible(true);

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"note\": \"vanilla's own ClientboundCommandsPacket bytes ")
           .append("for rustmc's command set, per permission level\",\n");
        out.append("  \"levels\": [\n");

        PermissionLevel[] levels = PermissionLevel.values();
        for (int i = 0; i < levels.length; i++) {
            PermissionLevel level = levels[i];
            CommandDispatcher<CommandSourceStack> dispatcher = new CommandDispatcher<>();
            registerOurs(dispatcher);

            // Exactly what `Commands.sendCommands` does, in its order.
            Map<CommandNode<CommandSourceStack>, CommandNode<CommandSourceStack>> mapping =
                new HashMap<>();
            RootCommandNode<CommandSourceStack> filtered = new RootCommandNode<>();
            mapping.put(dispatcher.getRoot(), filtered);
            CommandSourceStack source =
                Commands.createCompilationContext(LevelBasedPermissionSet.forLevel(level));
            fill.invoke(null, dispatcher.getRoot(), filtered, source, mapping);

            ClientboundCommandsPacket packet = new ClientboundCommandsPacket(filtered, inspector);
            FriendlyByteBuf buf = new FriendlyByteBuf(Unpooled.buffer());
            ClientboundCommandsPacket.STREAM_CODEC.encode(buf, packet);
            byte[] bytes = new byte[buf.readableBytes()];
            buf.getBytes(0, bytes);

            List<String> names = new ArrayList<>();
            collectLiterals(filtered, "", names);

            out.append("    {");
            out.append(" \"level\": \"").append(level.name()).append('"');
            out.append(", \"ordinal\": ").append(level.ordinal());
            out.append(", \"entityEventByte\": ").append(24 + level.ordinal());
            out.append(", \"usable\": [");
            for (int k = 0; k < names.size(); k++) {
                if (k > 0) {
                    out.append(", ");
                }
                out.append('"').append(names.get(k)).append('"');
            }
            out.append(']');
            out.append(", \"packetLen\": ").append(bytes.length);
            out.append(", \"packetHex\": \"").append(hex(bytes)).append('"');
            // `/help`'s answer at this level, straight from
            // `CommandDispatcher.getSmartUsage(root, source)`. Vanilla's own
            // usage generator over vanilla's own copy of OUR tree, which is the
            // only way to gate a `getSmartUsage` transcription without
            // reimplementing it in the check.
            out.append(", \"help\": [");
            var usage = dispatcher.getSmartUsage(dispatcher.getRoot(), source).values();
            int u = 0;
            for (String line : usage) {
                if (u++ > 0) {
                    out.append(", ");
                }
                out.append('"').append(line.replace("\\", "\\\\").replace("\"", "\\\"")).append('"');
            }
            out.append("]");
            out.append(" }");
            if (i < levels.length - 1) {
                out.append(',');
            }
            out.append('\n');
        }
        out.append("  ],\n");

        // --- THE ANSWER TO EVERY COMMAND THAT DOES NOT RUN ---------------
        //
        // `Commands.finishParsing` is what a real server sends when a parse
        // fails: `ComponentUtils.fromMessage(e.getRawMessage())` through
        // `sendFailure`, and then a second component carrying ten characters of
        // context, the offending remainder in red-underline, and
        // `command.context.here` in red-italic. It is ONE reply shape and it is
        // the observable answer for every typo, every unported root and every
        // permission refusal, which is why it is worth more than any single
        // command.
        //
        // These are vanilla's own components, not a reconstruction: the source
        // is a real `CommandSourceStack` with a recording `CommandSource`, and
        // `finishParsing` is invoked by reflection because it is private. A
        // golden that rebuilt the components here would be the port's own
        // arithmetic checked against itself.
        Method finish = Commands.class.getDeclaredMethod(
            "finishParsing",
            com.mojang.brigadier.ParseResults.class, String.class, CommandSourceStack.class);
        finish.setAccessible(true);
        CommandDispatcher<CommandSourceStack> failDispatcher = new CommandDispatcher<>();
        registerOurs(failDispatcher);
        out.append("  \"failures\": [\n");
        boolean firstFailure = true;
        for (PermissionLevel level : PermissionLevel.values()) {
            for (String input : FAILING_INPUTS) {
                List<Component> got = new ArrayList<>();
                CommandSourceStack src =
                    Commands.createCompilationContext(LevelBasedPermissionSet.forLevel(level))
                            .withSource(new RecordingSource(got));
                try {
                    finish.invoke(null, failDispatcher.parse(input, src), input, src);
                } catch (java.lang.reflect.InvocationTargetException e) {
                    throw new IllegalStateException("finishParsing threw for " + input, e);
                }
                if (!firstFailure) {
                    out.append(",\n");
                }
                firstFailure = false;
                out.append("    { \"level\": \"").append(level.name()).append('"');
                out.append(", \"input\": \"").append(input.replace("\\", "\\\\").replace("\"", "\\\"")).append('"');
                out.append(", \"lines\": [");
                for (int k = 0; k < got.size(); k++) {
                    if (k > 0) {
                        out.append(", ");
                    }
                    out.append('"').append(hex(encodeComponent(got.get(k)))).append('"');
                }
                out.append("] }");
            }
        }
        out.append("\n  ],\n");

        out.append("  \"replies\": {\n");
        appendExecutorReplies(out);
        appendReply(out, "listUuids", listUuidsReply(CORPUS_PLAYERS, MAX_PLAYERS), true);
        appendReply(out, "seed", seedReply(SEED), true);
        // A second seed, purely so the golden cannot be a constant. If the
        // encoder ever stopped reading its argument these two would be equal
        // and `ci/commands-golden.sh` says so.
        appendReply(out, "seedAlt", seedReply(SEED_ALT), true);
        // `/gamerule`'s two replies. Both are
        // `Component.translatable(key, rule.id(), rule.serialize(value))` —
        // `GameRuleCommand.queryRule` and `setRule` — and both arguments are
        // plain Strings, which `Component.translationArg` wraps as literal
        // components and which therefore serialise as bare TAG_Strings.
        //
        // A boolean rule and an integer one, because `GameRule.serialize` is
        // per-type and a golden taken on one of them would not see the other.
        appendReply(out, "gameruleQueryBool",
            Component.translatable("commands.gamerule.query", "keep_inventory", "false"), true);
        appendReply(out, "gameruleSetBool",
            Component.translatable("commands.gamerule.set", "keep_inventory", "true"), true);
        appendReply(out, "gameruleQueryInt",
            Component.translatable("commands.gamerule.query", "random_tick_speed", "3"), true);
        appendReply(out, "gameruleSetInt",
            Component.translatable("commands.gamerule.set", "random_tick_speed", "7"), true);
        // `/worldborder`'s nine replies. Every argument here goes through
        // `String.format(Locale.ROOT, "%.Nf", …)` — the SAME expression
        // `WorldBorderCommand` uses, so this golden gates the port's
        // formatter and not just its component shape.
        //
        // The values are chosen to separate Java's rounding from Rust's.
        // Java's `%f` applies HALF_UP to `FloatingDecimal`'s shortest
        // round-trip digits; Rust's `{:.N}` applies half-to-EVEN to the exact
        // binary value. `0.15` at one place, `0.125` at two and `2.5` at zero
        // disagree between the two, so a port that reached for `{:.1}` fails
        // three of these rows rather than passing by luck.
        appendReply(out, "worldborderSetImmediate",
            Component.translatable("commands.worldborder.set.immediate",
                String.format(Locale.ROOT, "%.1f", 100.0D)), true);
        appendReply(out, "worldborderSetImmediateTie",
            Component.translatable("commands.worldborder.set.immediate",
                String.format(Locale.ROOT, "%.1f", 0.15D)), true);
        appendReply(out, "worldborderSetShrink",
            Component.translatable("commands.worldborder.set.shrink",
                String.format(Locale.ROOT, "%.1f", 12.0D),
                String.format(Locale.ROOT, "%.2f", 20L / 20.0D)), true);
        appendReply(out, "worldborderSetGrow",
            Component.translatable("commands.worldborder.set.grow",
                String.format(Locale.ROOT, "%.1f", 200.0D),
                String.format(Locale.ROOT, "%.2f", 7L / 20.0D)), true);
        appendReply(out, "worldborderCenter",
            Component.translatable("commands.worldborder.center.success",
                String.format(Locale.ROOT, "%.2f", 137.5F),
                String.format(Locale.ROOT, "%.2f", -41.5F)), true);
        appendReply(out, "worldborderGet",
            Component.translatable("commands.worldborder.get",
                String.format(Locale.ROOT, "%.0f", 2.5D)), true);
        // The ONE reply whose argument is a primitive rather than a string,
        // which `ListTag.write` leaves as a bare TAG_Int because nothing in
        // its `with` list disagrees with it.
        appendReply(out, "worldborderWarningDistance",
            Component.translatable("commands.worldborder.warning.distance.success",
                Integer.valueOf(9)), true);
        appendReply(out, "worldborderWarningTime",
            Component.translatable("commands.worldborder.warning.time.success",
                String.format(Locale.ROOT, "%.2f", 37L / 20.0D)), true);
        appendReply(out, "worldborderDamageAmount",
            Component.translatable("commands.worldborder.damage.amount.success",
                String.format(Locale.ROOT, "%.2f", 0.125F)), true);
        appendReply(out, "worldborderDamageBuffer",
            Component.translatable("commands.worldborder.damage.buffer.success",
                String.format(Locale.ROOT, "%.2f", 1.25F)), true);
        // `/scoreboard objectives`' four replies, built from a REAL
        // `Objective` on a real `Scoreboard` rather than from a reconstruction.
        //
        // That matters for exactly one of the four arguments:
        // `Objective.getFormattedDisplayName()` is not the display name. It is
        // `ComponentUtils.wrapInSquareBrackets(displayName.copy().withStyle(…))`
        // — a `chat.square_brackets` translation whose single argument carries
        // a style the objective supplies — and a port that passed the plain
        // display name produces a component that renders almost right and
        // encodes nothing like it.
        //
        // `display.set`'s two arguments are of DIFFERENT kinds, which is the
        // other thing worth a vector here: the slot is a `String` and the
        // objective is a `Component`, so `ListTag.write`'s heterogeneous rule
        // wraps both. `display.cleared`'s single `String` is not wrapped.
        Scoreboard board = new Scoreboard();
        Objective obj = board.addObjective(
            "kills", ObjectiveCriteria.DUMMY, Component.literal("kills"),
            ObjectiveCriteria.RenderType.INTEGER, false, null);
        appendReply(out, "scoreboardObjectiveAdd",
            Component.translatable("commands.scoreboard.objectives.add.success",
                obj.getFormattedDisplayName()), true);
        appendReply(out, "scoreboardObjectiveRemove",
            Component.translatable("commands.scoreboard.objectives.remove.success",
                obj.getFormattedDisplayName()), true);
        appendReply(out, "scoreboardDisplaySet",
            Component.translatable("commands.scoreboard.objectives.display.set",
                DisplaySlot.SIDEBAR.getSerializedName(), obj.getDisplayName()), true);
        appendReply(out, "scoreboardDisplayCleared",
            Component.translatable("commands.scoreboard.objectives.display.cleared",
                DisplaySlot.SIDEBAR.getSerializedName()), true);
        appendReply(out, "scoreboardListEmpty",
            Component.translatable("commands.scoreboard.objectives.list.empty"), true);
        appendReply(out, "scoreboardListOne",
            Component.translatable("commands.scoreboard.objectives.list.success",
                Integer.valueOf(1),
                ComponentUtils.formatList(java.util.List.of(obj),
                    Objective::getFormattedDisplayName)), false);
        out.append("  },\n");
        // `/version` is the only reply here that vanilla hands over whole.
        // `VersionCommand.dumpVersion` is public and takes the consumer it
        // feeds one component per line, so these are not reconstructed from a
        // reading of the command -- they are the components themselves, and
        // every value in them comes from the pinned jar's `version.json`.
        // That is why `/version` is portable at all: it needs no world, no
        // player and no server state.
        // THE HEADER IS PART OF THE REPLY AND `dumpVersion` DOES NOT EMIT IT.
        // `VersionCommand`'s executor is `source.sendSystemMessage(HEADER)`
        // followed by `dumpVersion(source::sendSystemMessage)`, so a port
        // gated only against `dumpVersion` would send nine of vanilla's ten
        // `system_chat` packets and pass. `HEADER` is a private static, so it
        // is rebuilt here from the translation key its `<clinit>` uses.
        List<Component> versionLines = new ArrayList<>();
        versionLines.add(Component.translatable("commands.version.header"));
        VersionCommand.dumpVersion(versionLines::add);
        out.append("  \"versionLines\": [\n");
        for (int i = 0; i < versionLines.size(); i++) {
            out.append("    \"").append(hex(encodeComponent(versionLines.get(i)))).append('"');
            if (i < versionLines.size() - 1) {
                out.append(',');
            }
            out.append('\n');
        }
        out.append("  ],\n");
        out.append("  \"replyInputs\": { \"players\": [\"")
           .append(String.join("\", \"", CORPUS_PLAYERS))
           .append("\"], \"maxPlayers\": ").append(MAX_PLAYERS)
           .append(", \"seed\": ").append(SEED)
           .append(", \"seedAlt\": ").append(SEED_ALT)
           .append(" }\n");
        out.append("}\n");
        System.out.print(out);
    }

    /// The two names `tools/oracle-runner` connects with, in join order, and
    /// the `max-players` `crate::status` carries. Fixed here so the Rust side
    /// has something to reproduce.
    /// The inputs `failures` is taken on, chosen so that every arm of
    /// `getParseException` and every branch of the context component is
    /// reached at least once.
    ///
    ///   nope                 nothing matched          -> unknown command
    ///   list oops            a prefix matched         -> unknown argument
    ///   list  uuids          a doubled separator      -> expected separator
    ///   gamerule keep_inventory yes   one candidate raised -> that exception
    ///   gamerule keep_inventory 5     ditto, a different reader error
    ///   seed                 refused by permission below GAMEMASTERS, and a
    ///                        plain success above it, so the SAME input
    ///                        produces output at one level and none at another
    ///   help                 a success, which must produce NO failure lines
    ///   a very long unknown command   `i > 10`, the ellipsis branch
    private static final String[] FAILING_INPUTS = {
        "nope",
        "list oops",
        "list  uuids",
        "gamerule keep_inventory yes",
        "gamerule keep_inventory 5",
        "seed",
        "help",
        "definitely_not_a_command_at_all",
        "gamerule",
        "msg",
        "tell",
        // The argument-level arm of `getParseException`, one input per
        // `BrigadierExceptions` shape our own tree can actually reach:
        // a 0-argument reader error, a 1-argument one, and both 2-argument
        // bound errors. `/gamerule`'s value nodes are the only place this
        // server has an argument a player can get wrong.
        "gamerule random_tick_speed x",     // parsing.int.expected      (0 args)
        "gamerule random_tick_speed 1.5",   // parsing.int.invalid       (1 arg)
        "gamerule random_tick_speed -1",    // argument.integer.low      (2 args)
        "gamerule max_snow_accumulation_height 99", // argument.integer.big
        "gamerule advance_time 5",          // parsing.bool.invalid      (1 arg)
        // The roots `crate::cmdexec` executes, so each one's permission
        // pruning is compared at all five levels: a level-0 source must get
        // the same unknown-command reply a typo gets, and a level-2 one the
        // reply that names the argument it stopped at.
        "difficulty impossible",            // command.unknown.argument at 2+
        "gamemode",                         // incomplete: the root has no Command
        "tp",                               // a redirect with nothing after it
        "xp add",                           // incomplete two literals deep
        "execute as",                       // a fork with nothing to fork over
        "execute run",                      // a root redirect with nothing after it
        "random value 5..1",                // argument.range.swapped, at level 0 too
        "random roll",                      // incomplete
    };

    /// A `CommandSource` that keeps what it is sent.
    ///
    /// `CommandSource.NULL` accepts neither success nor failure, so
    /// `sendFailure` returns without doing anything and `finishParsing` looks
    /// silent. `acceptsFailure()` must be true here or this whole dump is
    /// empty lists, which is the shape of a golden that cannot fail.
    private record RecordingSource(List<Component> into) implements CommandSource {
        @Override public void sendSystemMessage(Component c) { into.add(c); }
        @Override public boolean acceptsSuccess() { return true; }
        @Override public boolean acceptsFailure() { return true; }
        @Override public boolean shouldInformAdmins() { return false; }
    }

    private static final List<String> CORPUS_PLAYERS = List.of("oracle", "oracle_eye");
    private static final int MAX_PLAYERS = 20;
    private static final long SEED = 20260722L;
    private static final long SEED_ALT = -8_675_309L;

    /// `ListPlayersCommand.listPlayersWithUuids` — `commands.list.players`
    /// with the joined list built from `commands.list.nameAndId` rather than
    /// from display names.
    ///
    /// `Player.getName()` is `Component.literal(profile.name())` and
    /// `Component.translationArg(UUID)` is `Component.literal(uuid.toString())`,
    /// so unlike the plain `/list` reply this one needs no live player and can
    /// be built here exactly as vanilla builds it.
    private static Component listUuidsReply(List<String> names, int maxPlayers) {
        Component joined = ComponentUtils.formatList(names, name ->
            Component.translatable(
                "commands.list.nameAndId",
                Component.literal(name),
                Component.translationArg(UUIDUtil.createOfflinePlayerUUID(name))));
        return Component.translatable(
            "commands.list.players", names.size(), maxPlayers, joined);
    }

    /// `SeedCommand`: `commands.seed.success` around
    /// `ComponentUtils.copyOnClickText(String.valueOf(seed))`.
    private static Component seedReply(long seed) {
        return Component.translatable(
            "commands.seed.success", ComponentUtils.copyOnClickText(String.valueOf(seed)));
    }

    /// A whole `ClientboundSystemChatPacket` body: the component in network
    /// NBT, then the `overlay` boolean, which `sendSuccess` leaves false.
    ///
    /// `TRUSTED_CONTEXT_FREE_STREAM_CODEC` is used rather than
    /// `TRUSTED_STREAM_CODEC` because it is a `StreamCodec<ByteBuf, ...>` and
    /// so needs no `RegistryFriendlyByteBuf` — no registry access, no
    /// registry load, and nothing ambient for the double run to catch.
    /// A `system_chat` body: the component in network form, then `overlay`.
    private static byte[] encodeComponent(Component c) {
        FriendlyByteBuf buf = new FriendlyByteBuf(Unpooled.buffer());
        ComponentSerialization.TRUSTED_CONTEXT_FREE_STREAM_CODEC.encode(buf, c);
        buf.writeBoolean(false);
        byte[] bytes = new byte[buf.readableBytes()];
        buf.getBytes(0, bytes);
        return bytes;
    }

    /// The replies `crate::cmdexec`'s executors send, each built by VANILLA'S
    /// OWN CODE wherever the jar lets it be called: a message lambda invoked
    /// by reflection, an exception raised from the command's own static
    /// exception type, and `sendFailure` run on a real `CommandSourceStack`.
    /// Only `broadcastToAdmins`' line is rebuilt, because the method needs a
    /// live `MinecraftServer` — and it is rebuilt from the same three calls
    /// its bytecode makes (`chat.type.admin`, `getDisplayName`, `GRAY, ITALIC`).
    ///
    /// Every row is written with a trailing comma; the existing rows that
    /// follow carry the map's closing one.
    private static void appendExecutorReplies(StringBuilder out) throws Exception {
        for (Difficulty d : Difficulty.values()) {
            String n = d.getSerializedName();
            appendReply(out, "difficultySuccess_" + n,
                lambda(DifficultyCommand.class, "lambda$setDifficulty$0", d), true);
            appendReply(out, "difficultyQuery_" + n,
                lambda(DifficultyCommand.class, "lambda$register$2", d), true);
            appendReply(out, "difficultyFailure_" + n,
                raised(DifficultyCommand.class, "ERROR_ALREADY_SAME_DIFFICULTY", d.getDisplayName()),
                true);
            appendReply(out, "difficultyFailureLine_" + n,
                failureLine(raised(DifficultyCommand.class, "ERROR_ALREADY_SAME_DIFFICULTY",
                    d.getDisplayName())), true);
        }
        for (GameType t : GameType.values()) {
            Component mode = Component.translatable("gameMode." + t.getName());
            appendReply(out, "gamemodeSelf_" + t.getName(),
                lambda(GameModeCommand.class, "lambda$logGamemodeChange$0", mode), true);
            // `ServerPlayer.getDisplayName()` needs a live player, so the
            // argument is the display name `crate::commands` already gates
            // against the recorded `/list` reply, rebuilt here from the same
            // calls `Player.decorateDisplayNameComponent` makes.
            appendReply(out, "gamemodeChanged_" + t.getName(),
                Component.translatable("gameMode.changed", mode), true);
        }
        for (GameType t : GameType.values()) {
            appendReply(out, "defaultGameMode_" + t.getName(),
                lambda(DefaultGameModeCommands.class, "lambda$setMode$0", t), true);
        }
        for (String m : List.of("lambda$clearTitle$1", "lambda$resetTitle$1", "lambda$setTimes$1")) {
            appendReply(out, "title_" + m, lambda(TitleCommand.class, m, List.of(1, 2)), true);
        }
        // `/time`'s replies. `Holder.getRegisteredName()` needs the
        // `world_clock` registry, which this golden does not load, so the
        // clock name is the literal string it would return; what is gated is
        // the ARGUMENT TYPES — an `int` for `set` (`Integer.valueOf`) and a
        // `long` for `add` and `query` (`Long.valueOf`) — which encode as
        // different tags.
        appendReply(out, "timeSet", Component.translatable("commands.time.set.absolute",
            "minecraft:overworld", Integer.valueOf(6000)), true);
        appendReply(out, "timeAdd", Component.translatable("commands.time.set.absolute",
            "minecraft:overworld", Long.valueOf(30000L)), true);
        appendReply(out, "timeQuery", Component.translatable("commands.time.query.absolute",
            "minecraft:overworld", Long.valueOf(6000L)), true);
        appendReply(out, "timeQueryGame", lambda(TimeCommand.class, "lambda$queryGameTime$0", 1234567L), true);
        appendReply(out, "setblockSuccess",
            lambda(SetBlockCommand.class, "lambda$setBlock$0", new net.minecraft.core.BlockPos(-7, 64, 12)), true);
        appendReply(out, "setblockFailed",
            raised(SetBlockCommand.class, "ERROR_FAILED"), true);
        appendReply(out, "posUnloaded",
            raised(net.minecraft.commands.arguments.coordinates.BlockPosArgument.class, "ERROR_NOT_LOADED"), true);
        appendReply(out, "posOutOfWorld",
            raised(net.minecraft.commands.arguments.coordinates.BlockPosArgument.class, "ERROR_OUT_OF_WORLD"), true);
        appendReply(out, "fillSuccess",
            lambda(net.minecraft.server.commands.FillCommand.class, "lambda$fillBlocks$0", 27), true);
        appendReply(out, "fillFailed",
            raised(net.minecraft.server.commands.FillCommand.class, "ERROR_FAILED"), true);
        appendReply(out, "fillTooBig",
            raised(net.minecraft.server.commands.FillCommand.class, "ERROR_AREA_TOO_LARGE",
                Integer.valueOf(32768), Long.valueOf(50000L)), true);
        appendReply(out, "playsoundMultiple",
            lambda(net.minecraft.server.commands.PlaySoundCommand.class, "lambda$playSound$1",
                net.minecraft.resources.Identifier.parse("minecraft:block.note_block.bell"), 2), true);
        appendReply(out, "playsoundFailed",
            raised(net.minecraft.server.commands.PlaySoundCommand.class, "ERROR_TOO_FAR"), true);
        {
            Class<?> sc = net.minecraft.server.commands.StopSoundCommand.class;
            var bell = net.minecraft.resources.Identifier.parse("minecraft:block.note_block.bell");
            var block = net.minecraft.sounds.SoundSource.BLOCKS;
            appendReply(out, "stopsoundSourceSound", lambda(sc, "lambda$stopSound$0", bell, block), true);
            appendReply(out, "stopsoundSourceAny", lambda(sc, "lambda$stopSound$1", block), true);
            appendReply(out, "stopsoundSound", lambda(sc, "lambda$stopSound$2", bell), true);
            appendReply(out, "stopsoundAny", lambda(sc, "lambda$stopSound$3"), true);
        }
        appendReply(out, "cloneSuccess",
            lambda(net.minecraft.server.commands.CloneCommands.class, "lambda$clone$0", 9), true);
        appendReply(out, "cloneOverlap",
            raised(net.minecraft.server.commands.CloneCommands.class, "ERROR_OVERLAP"), true);
        appendReply(out, "cloneFailed",
            raised(net.minecraft.server.commands.CloneCommands.class, "ERROR_FAILED"), true);
        appendReply(out, "cloneTooBig",
            raised(net.minecraft.server.commands.CloneCommands.class, "ERROR_AREA_TOO_LARGE",
                Integer.valueOf(32768), Long.valueOf(40000L)), true);
        for (String w : List.of("title", "subtitle", "actionbar")) {
            appendReply(out, "titleShow_" + w,
                lambda(TitleCommand.class, "lambda$showTitle$1", w, List.of(1, 2)), true);
        }
        appendReply(out, "randomSample",
            lambda(net.minecraft.server.commands.RandomCommand.class, "lambda$randomSample$0", 4), true);
        appendReply(out, "randomTooSmall",
            raised(net.minecraft.server.commands.RandomCommand.class, "ERROR_RANGE_TOO_SMALL"), true);
        appendReply(out, "randomTooLarge",
            raised(net.minecraft.server.commands.RandomCommand.class, "ERROR_RANGE_TOO_LARGE"), true);
        appendReply(out, "randomRollConsole", Component.translatable("commands.random.roll",
            Component.literal("Server"), Integer.valueOf(3), Integer.valueOf(1), Integer.valueOf(6)), true);
        // `/rotate` and `/swing`'s single replies take a live `Entity`, so
        // the display name is the one `gamemodeChanged_` above rebuilds; the
        // count reply, the failures and `/particle`'s replies are vanilla's
        // own code.
        appendReply(out, "rotateSuccess", Component.translatable("commands.rotate.success",
            Component.literal("oracle")), true);
        appendReply(out, "swingSingle", Component.translatable("commands.swing.success.single",
            Component.literal("oracle")), true);
        appendReply(out, "swingMultiple",
            lambda(net.minecraft.server.commands.SwingCommand.class, "lambda$swing$1", 2), true);
        appendReply(out, "swingNotLiving",
            raised(net.minecraft.server.commands.SwingCommand.class, "ERROR_NO_LIVING_ENTITY"), true);
        appendReply(out, "particleSuccess",
            lambda(net.minecraft.server.commands.ParticleCommand.class, "lambda$sendParticles$0",
                net.minecraft.core.particles.ParticleTypes.FLAME), true);
        appendReply(out, "particleFailed",
            raised(net.minecraft.server.commands.ParticleCommand.class, "ERROR_FAILED"), true);
        appendReply(out, "dialogClearSingle", Component.translatable("commands.dialog.clear.single",
            Component.literal("oracle")), true);
        appendReply(out, "dialogClearMultiple",
            lambda(net.minecraft.server.commands.DialogCommand.class, "lambda$clearDialog$1",
                List.of(1, 2)), true);
        {
            // A real `CustomBossEvent`, so every reply below is the command's
            // own lambda over vanilla's own `getDisplayName()`.
            Class<?> bb = net.minecraft.server.commands.BossBarCommands.class;
            net.minecraft.server.bossevents.CustomBossEvent bar =
                new net.minecraft.server.bossevents.CustomBossEvent(
                    new java.util.UUID(1L, 2L), net.minecraft.resources.Identifier.parse("minecraft:t"),
                    Component.literal("T"), () -> {});
            appendReply(out, "bossbarCreate", lambda(bb, "lambda$createBar$0", bar), true);
            appendReply(out, "bossbarRemove", lambda(bb, "lambda$removeBar$0", bar), true);
            appendReply(out, "bossbarListNone", lambda(bb, "lambda$listBars$0"), true);
            appendReply(out, "bossbarListOne", lambda(bb, "lambda$listBars$1", List.of(bar)), true);
            appendReply(out, "bossbarGetValue", lambda(bb, "lambda$getValue$0", bar), true);
            appendReply(out, "bossbarGetMax", lambda(bb, "lambda$getMax$0", bar), true);
            appendReply(out, "bossbarGetVisible", lambda(bb, "lambda$getVisible$0", bar), true);
            appendReply(out, "bossbarGetPlayersNone", lambda(bb, "lambda$getPlayers$0", bar), true);
            appendReply(out, "bossbarSetPlayersNone", lambda(bb, "lambda$setPlayers$0", bar), true);
            appendReply(out, "bossbarSetValue", lambda(bb, "lambda$setValue$0", bar, 50), true);
            appendReply(out, "bossbarSetMax", lambda(bb, "lambda$setMax$0", bar, 200), true);
            appendReply(out, "bossbarSetName", lambda(bb, "lambda$setName$0", bar), true);
            bar.setColor(net.minecraft.world.BossEvent.BossBarColor.RED);
            appendReply(out, "bossbarSetColorRed", lambda(bb, "lambda$setColor$0", bar), true);
            bar.setColor(net.minecraft.world.BossEvent.BossBarColor.PURPLE);
            appendReply(out, "bossbarSetColorPurple", lambda(bb, "lambda$setColor$0", bar), true);
            bar.setVisible(false);
            appendReply(out, "bossbarSetHidden", lambda(bb, "lambda$setVisible$1", bar), true);
            appendReply(out, "bossbarGetHidden", lambda(bb, "lambda$getVisible$1", bar), true);
            appendReply(out, "bossbarExists", raised(bb, "ERROR_ALREADY_EXISTS", "minecraft:t"), true);
            appendReply(out, "bossbarUnknown", raised(bb, "ERROR_DOESNT_EXIST", "minecraft:t"), true);
            for (String f : List.of("ERROR_NO_PLAYER_CHANGE", "ERROR_NO_NAME_CHANGE",
                    "ERROR_NO_COLOR_CHANGE", "ERROR_NO_STYLE_CHANGE", "ERROR_NO_VALUE_CHANGE",
                    "ERROR_NO_MAX_CHANGE", "ERROR_ALREADY_HIDDEN", "ERROR_ALREADY_VISIBLE")) {
                appendReply(out, "bossbar_" + f, raised(bb, f), true);
            }
        }
        appendReply(out, "teammsgNoTeam",
            raised(net.minecraft.server.commands.TeamMsgCommand.class, "ERROR_NOT_ON_TEAM"), true);
        appendReply(out, "notPlayer",
            raised(CommandSourceStack.class, "ERROR_NOT_PLAYER"), true);
        appendReply(out, "noPlayersFound",
            raised(EntityArgument.class, "NO_PLAYERS_FOUND"), true);
        appendReply(out, "noEntitiesFound",
            raised(EntityArgument.class, "NO_ENTITIES_FOUND"), true);
        appendReply(out, "notEntity",
            raised(CommandSourceStack.class, "ERROR_NOT_ENTITY"), true);
        appendReply(out, "teleportInvalidPosition",
            raised(TeleportCommand.class, "INVALID_POSITION"), true);
        // The location replies through `lambda$teleportToPos$1` (the count
        // form, which needs no live entity) at values chosen to separate
        // Java's HALF_UP `%f` from Rust's half-to-even: 0.0000005 rounds UP to
        // 0.000001 in Java.
        appendReply(out, "teleportLocationMultiple",
            lambda(TeleportCommand.class, "lambda$teleportToPos$1",
                List.of(1, 2), new net.minecraft.world.phys.Vec3(12.5, -60.0, 0.0000005)), true);
        for (String arm : List.of("clear", "rain", "thunder")) {
            appendReply(out, "weather_" + arm,
                lambda(WeatherCommand.class, "lambda$set" + Character.toUpperCase(arm.charAt(0))
                    + arm.substring(1) + "$0"), true);
        }
        // `/experience`'s count-form replies through the command's own
        // lambdas. The `Type` argument is the nested enum's constant, looked up
        // by name so this file need not name the nested class.
        Class<?> xpType = Class.forName("net.minecraft.server.commands.ExperienceCommand$Type");
        for (Object t : xpType.getEnumConstants()) {
            String n = ((Enum<?>) t).name().toLowerCase(Locale.ROOT);
            appendReply(out, "experienceAddMultiple_" + n,
                lambda(ExperienceCommand.class, "lambda$addExperience$1", t, 7, List.of(1, 2)), true);
            appendReply(out, "experienceSetMultiple_" + n,
                lambda(ExperienceCommand.class, "lambda$setExperience$1", t, 0, List.of(1, 2)), true);
        }
        appendReply(out, "experienceSetPointsInvalid",
            raised(ExperienceCommand.class, "ERROR_SET_POINTS_INVALID"), true);
        appendReply(out, "adminLineConsoleDifficulty",
            Component.translatable("chat.type.admin", Component.literal("Server"),
                lambda(DifficultyCommand.class, "lambda$setDifficulty$0", Difficulty.HARD))
                .withStyle(ChatFormatting.GRAY, ChatFormatting.ITALIC), true);
    }

    /// A private static method of a command class, by name and arity — the
    /// `sendSuccess` suppliers are `lambda$...` methods and calling them is
    /// calling vanilla's component code rather than reconstructing it.
    private static Component lambda(Class<?> owner, String name, Object... args) throws Exception {
        for (Method m : owner.getDeclaredMethods()) {
            if (m.getName().equals(name) && m.getParameterCount() == args.length) {
                m.setAccessible(true);
                return (Component) m.invoke(null, args);
            }
        }
        throw new IllegalStateException("no " + owner.getSimpleName() + "." + name + "/" + args.length);
    }

    /// An executor's `CommandSyntaxException`, raised from the command's own
    /// static exception type, as the raw message `handleError` sends on.
    private static Component raised(Class<?> owner, String field, Object... args) throws Exception {
        Field f = owner.getDeclaredField(field);
        f.setAccessible(true);
        Object type = f.get(null);
        CommandSyntaxException e;
        if (type instanceof SimpleCommandExceptionType t) {
            e = t.create();
        } else if (type instanceof DynamicCommandExceptionType t) {
            e = t.create(args[0]);
        } else if (type instanceof Dynamic2CommandExceptionType t) {
            e = t.create(args[0], args[1]);
        } else {
            throw new IllegalStateException(owner.getSimpleName() + "." + field + " is a " + type.getClass());
        }
        return ComponentUtils.fromMessage(e.getRawMessage());
    }

    /// `CommandSourceStack.sendFailure` itself, on a real stack whose source
    /// records what it is sent.
    private static Component failureLine(Component raw) {
        List<Component> got = new ArrayList<>();
        Commands.createCompilationContext(LevelBasedPermissionSet.OWNER)
            .withSource(new RecordingSource(got))
            .sendFailure(raw);
        if (got.size() != 1) {
            throw new IllegalStateException("sendFailure sent " + got.size() + " components");
        }
        return got.get(0);
    }

    private static void appendReply(StringBuilder out, String name, Component c, boolean comma) {
        byte[] bytes = encodeComponent(c);
        out.append("    \"").append(name).append("\": \"").append(hex(bytes)).append('"');
        if (comma) {
            out.append(',');
        }
        out.append('\n');
    }

    /// Every executable literal path in the filtered tree, for a
    /// human-readable column beside the hex. Sorted by nothing: the walk is
    /// the tree's own child order, which is the order under test.
    private static void collectLiterals(
            CommandNode<CommandSourceStack> node, String prefix, List<String> into) {
        for (CommandNode<CommandSourceStack> child : node.getChildren()) {
            String path = prefix.isEmpty() ? child.getName() : prefix + " " + child.getName();
            if (child.getCommand() != null) {
                into.add(path);
            }
            collectLiterals(child, path, into);
        }
    }

    private static String hex(byte[] b) {
        StringBuilder s = new StringBuilder(b.length * 2);
        for (byte x : b) {
            s.append(Character.forDigit((x >> 4) & 0xF, 16));
            s.append(Character.forDigit(x & 0xF, 16));
        }
        return s.toString();
    }
}
