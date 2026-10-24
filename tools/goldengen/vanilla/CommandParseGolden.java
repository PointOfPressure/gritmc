// CommandParseGolden — what vanilla's own argument parsers accept, reject, and
// how far they move the cursor.
//
// # The question this answers, which no other dumper here asks
//
// `CommandTreeExtract` proves this server can rebuild vanilla's declare-commands
// packet. That is a claim about SHAPE, and its own header says so: a tree with
// every node placed and every property byte right autocompletes perfectly and
// does nothing. The next thing a port needs is the other half — given a
// command string, which node consumes which characters — and nothing in the
// tree dump constrains it at all.
//
// It is worth its own gate because getting it wrong is silent. A parser that
// consumes one character too many leaves the next node looking at the wrong
// input, and the failure surfaces as "unknown command" for a command that is
// present, spelled right, and whose executor works. A parser that consumes one
// too few leaves a `dispatcherExpectedArgumentSeparator` that reads the same
// way. Neither is visible to a test that only asks "did this string parse".
//
// So the recorded quantity is the CURSOR, not the verdict:
//
//   { parser, props, input, accepted, cursor, error }
//
// `cursor` is `StringReader.getCursor()` after `ArgumentType.parse` returns, or
// after it throws — Brigadier's own readers rewind before raising in some cases
// and not in others, and which is which is exactly the kind of thing a port
// invents rather than reads. `error` is the `CommandSyntaxException`'s type
// name, kept for diagnosis and NOT compared: this server does not render
// Brigadier's messages (see `crates/rustmc-server/src/commands.rs`'s header),
// so demanding the right exception would be gating a behaviour nothing ships.
//
// # Why the corpus is in this file
//
// The inputs are a constant below rather than an argument, because a golden
// whose corpus the reader chooses is a golden the reader can make pass. They
// are grouped by what each is meant to separate — bounds, quoting, escapes,
// the greedy/word/phrase split, the characters that end an unquoted run — and
// every group has at least one case that must be REJECTED, because a corpus of
// only-valid inputs cannot catch a parser that accepts everything.
//
// # What this covers, and what it cannot
//
// Brigadier's six primitives and `brigadier:string`, plus every
// `net.minecraft.commands.arguments` type whose PARSE needs no live level. That
// second group is larger than it looks: an entity selector, a block position, a
// block state and an item stack are all parsed before anything is resolved, so
// `EntityArgument.players().parse(reader)` on `@e[type=pig]` succeeds against no
// world at all and fails on `@q` for a reason that is purely syntactic. A build
// context IS needed by `minecraft:block_state` and `minecraft:item_stack`, and
// one is made here exactly as `CommandTreeExtract` makes it.
//
// What is genuinely out of reach is a parse whose success depends on server
// state — `minecraft:function`, `minecraft:objective`, the loot parsers. None of
// those is a parser this port needs for a command it can reach, so the omission
// costs nothing today; when it does, it will cost a live level and not a row.
//
// **A row here is not a claim that the port implements the parser.** The Rust
// side buckets each row three ways — agreed, disagreed, and "this server cannot
// read this parser" — and only the middle bucket is a failure. The third is a
// ratcheted count, which is what makes this file a coverage measurement as well
// as a gate.
//
// Output: JSON to the file named by `-Drustmc.parse.out`. Not stdout, because
// `Bootstrap.bootStrap()` re-emits stdout through log4j with a prefix.

package net.minecraft.commands;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.mojang.brigadier.StringReader;
import com.mojang.brigadier.arguments.ArgumentType;
import com.mojang.brigadier.arguments.BoolArgumentType;
import com.mojang.brigadier.arguments.DoubleArgumentType;
import com.mojang.brigadier.arguments.FloatArgumentType;
import com.mojang.brigadier.arguments.IntegerArgumentType;
import com.mojang.brigadier.arguments.LongArgumentType;
import com.mojang.brigadier.arguments.StringArgumentType;
import com.mojang.brigadier.exceptions.CommandSyntaxException;

import net.minecraft.commands.arguments.MessageArgument;
import net.minecraft.commands.arguments.IdentifierArgument;
import net.minecraft.commands.arguments.TimeArgument;
import net.minecraft.commands.arguments.blocks.BlockStateArgument;
import net.minecraft.commands.arguments.coordinates.BlockPosArgument;
import net.minecraft.commands.arguments.ObjectiveArgument;
import net.minecraft.commands.arguments.ObjectiveCriteriaArgument;
import net.minecraft.commands.arguments.ScoreboardSlotArgument;
import net.minecraft.commands.arguments.coordinates.Vec2Argument;
import net.minecraft.commands.arguments.coordinates.Vec3Argument;
import net.minecraft.commands.arguments.item.ItemArgument;
import net.minecraft.commands.arguments.EntityArgument;
import net.minecraft.commands.arguments.GameModeArgument;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.world.flag.FeatureFlags;

import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;

public final class CommandParseGolden {

    /// One row of the corpus: how to build the type, and what to feed it.
    private static JsonObject props(Object... kv) {
        JsonObject o = new JsonObject();
        for (int i = 0; i < kv.length; i += 2) {
            String k = (String) kv[i];
            Object v = kv[i + 1];
            if (v instanceof Number n) {
                o.addProperty(k, n);
            } else {
                o.addProperty(k, String.valueOf(v));
            }
        }
        return o;
    }

    /// The inputs every parser is fed, whatever it is.
    ///
    /// Shared rather than per-parser so that the corpus asks each parser what
    /// it does with the OTHER parsers' inputs too — which is where a wrong
    /// cursor hides. A greedy string handed `1 2 3` and an integer handed
    /// `hello world` are both real positions in a real tree.
    private static final String[] COMMON = {
        "",
        " ",
        "0",
        "1",
        "-1",
        "8",
        "9",
        "42",
        "-42",
        "1.5",
        "-1.5",
        "1.2.3",
        ".5",
        "1.",
        "--1",
        "2147483647",
        "2147483648",
        "-2147483648",
        "9223372036854775807",
        "9223372036854775808",
        "1e3",
        "0x10",
        "+1",
        "true",
        "false",
        "TRUE",
        "yes",
        "\"true\"",
        "'false'",
        "hello",
        "hello world",
        "hello  world",
        "hello world ",
        " hello",
        "stone",
        "stone[facing=north]",
        "minecraft:stone",
        "a/b",
        "a:b",
        "a_b-c.d+e",
        "@a",
        "@e[type=pig]",
        "~ ~ ~",
        "1 2 3",
        "\"quoted phrase\"",
        "\"quoted phrase\" tail",
        "'single quoted'",
        "\"unterminated",
        "\"esc\\\"aped\"",
        "\"bad\\nescape\"",
        "\"\"",
        "''",
        "\"a\"b",
        "café",
        "café au lait",
        "~",
        "~ ~ ~",
        "~1 ~-2 ~",
        "^ ^ ^1",
        "1 2",
        "1.5 2.5 3.5",
        "@s",
        "@a",
        "@p",
        "@r",
        "@e",
        "@q",
        "@",
        "@e[type=pig]",
        "@e[",
        "@a[limit=1]",
        "Notch",
        "a@b.com",
        "hello@world",
        "1d",
        "5s",
        "10t",
        "1.5d",
        "-1t",
        "5x",
        "survival",
        "creative",
        "spectator",
        "adventure",
        "flying",
        "minecraft:stone[facing=north]",
        "stone{Foo:1b}",
        "#logs",
        "diamond_sword",
        "\"a\":\"b\"",
        "{\"text\":\"hi\"}",
        // `Identifier.read` accepts an EMPTY run — the empty identifier is a
        // valid parse — so without a malformed one this parser rejects nothing
        // on this corpus and the golden's own "must reject something" check
        // refuses the dump. Two colons is the cheapest malformed one:
        // `bySeparator` splits at the first, leaving `:` in the path, which is
        // outside the allowed path characters.
        "a:b:c",
        "foo:BAR",
        // `@n` is the SIXTH selector type and it is easy to miss: the
        // `tableswitch` in `parseSelector` covers 97..115 and `n` (110) is one
        // of the six live arms, alongside a, e, p, r and s.
        "@n",
        // `parseNameOrUUID` tries `UUID.fromString` FIRST and falls back to a
        // player name only on `IllegalArgumentException`, so the UUID grammar
        // is part of this parser's accept set and a port that skipped it would
        // be unfalsifiable here. Java's `fromString` is strict about having
        // exactly four dashes and at most 36 characters and lenient about the
        // width of each component, which is a combination nobody guesses.
        "069a79f4-44e9-4726-a5be-fca90e38aaf5",
        "a-b-c-d-e",
        "1-2-3-4-5-6",
        "----",
        "069a79f4-44e9-4726-a5be-fca90e38aaf5x",
        "0123456789abcdefg",
        "0123456789abcdef",
        "\"a name\"",
        "@s[",
        "@a[limit=1] tail",
        "hi @a there",
        "hi @q there",
        "hi @ there",
        // The scoreboard family. `ObjectiveCriteria.byName` has three arms and
        // these are one input each: a `CRITERIA_CACHE` hit that needs no
        // registry, a `killedByTeam.<colour>` from the per-team-colour family,
        // a name with no `:` that is a definite reject, and a stat form whose
        // accept is decided by `BuiltInRegistries.STAT_TYPE` and which this
        // port cannot read.
        "dummy",
        "killedByTeam.red",
        "notacriterion",
        "minecraft.mined:minecraft.stone",
        // `DisplaySlot`'s own names — one bare, one from the sixteen-strong
        // team family — because without an accepted row
        // `minecraft:scoreboard_slot` would be indistinguishable from a parser
        // that always fails and this dumper would refuse it.
        // `minecraft:int_range`'s own shapes: a point, both bounds, either
        // half, a swapped pair, the empty range and a malformed run.
        "1..5",
        "5..",
        "..5",
        "5..1",
        "..",
        "1-2..3",
        // `minecraft:particle`'s own shapes: a simple type bare and
        // namespaced, the same with an options tag, a type that needs
        // options, and an unknown name.
        "flame",
        "minecraft:flame",
        "flame{}",
        "dust",
        "minecraft:nope",
        "sidebar",
        "sidebar.team.red",
        "below_name",
    };

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        for (var pending : BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(lookup)) {
            pending.apply();
        }
        // The same context `CommandTreeExtract` builds the whole dispatcher
        // with: no world, no server, vanilla's feature set. `minecraft:block_state`
        // and `minecraft:item_stack` need it to know which blocks and items
        // exist; nothing else here does.
        CommandBuildContext ctx = CommandBuildContext.simple(lookup, FeatureFlags.VANILLA_SET);

        // The type constructions, paired with the properties a reader needs to
        // rebuild the same type. The bounds are chosen to be asymmetric and
        // small so that `9` and `-1` fall outside one of them: a bound the
        // corpus never crosses is a bound this file does not gate.
        record Spec(String parser, JsonObject props, ArgumentType<?> type) {}
        Spec[] specs = {
            new Spec("brigadier:bool", props(), BoolArgumentType.bool()),
            new Spec("brigadier:integer", props(), IntegerArgumentType.integer()),
            new Spec("brigadier:integer", props("min", 0),
                     IntegerArgumentType.integer(0)),
            new Spec("brigadier:integer", props("min", 0, "max", 8),
                     IntegerArgumentType.integer(0, 8)),
            new Spec("brigadier:integer", props("max", 8),
                     IntegerArgumentType.integer(Integer.MIN_VALUE, 8)),
            new Spec("brigadier:long", props(), LongArgumentType.longArg()),
            new Spec("brigadier:long", props("min", 0, "max", 8),
                     LongArgumentType.longArg(0, 8)),
            new Spec("brigadier:float", props(), FloatArgumentType.floatArg()),
            new Spec("brigadier:float", props("min", 1.0E-5f, "max", 1000.0f),
                     FloatArgumentType.floatArg(1.0E-5f, 1000.0f)),
            new Spec("brigadier:double", props(), DoubleArgumentType.doubleArg()),
            new Spec("brigadier:double", props("min", -1.5, "max", 1.5),
                     DoubleArgumentType.doubleArg(-1.5, 1.5)),
            new Spec("brigadier:string", props("type", "word"), StringArgumentType.word()),
            new Spec("brigadier:string", props("type", "phrase"), StringArgumentType.string()),
            new Spec("brigadier:string", props("type", "greedy"),
                     StringArgumentType.greedyString()),
            // `minecraft:message` — the parser `/say`, `/me`, `/msg` and
            // `/teammsg` all take. Greedy, capped at 256 remaining characters,
            // and it scans the text for `@` selectors as it goes.
            new Spec("minecraft:message", props(), MessageArgument.message()),
            // The four `minecraft:entity` shapes, which differ only in the two
            // flag bits the wire carries. Their PARSE is identical; the
            // difference shows up as `ERROR_ONLY_SINGLE_ALLOWED` and
            // `ERROR_ONLY_PLAYERS_ALLOWED` after the selector is read, which is
            // exactly the kind of thing a port assumes away.
            new Spec("minecraft:entity", props("amount", "single", "type", "entities"),
                     EntityArgument.entity()),
            new Spec("minecraft:entity", props("amount", "multiple", "type", "entities"),
                     EntityArgument.entities()),
            new Spec("minecraft:entity", props("amount", "single", "type", "players"),
                     EntityArgument.player()),
            new Spec("minecraft:entity", props("amount", "multiple", "type", "players"),
                     EntityArgument.players()),
            new Spec("minecraft:block_pos", props(), BlockPosArgument.blockPos()),
            new Spec("minecraft:vec3", props(), Vec3Argument.vec3()),
            // `minecraft:vec2` — `/worldborder center`'s argument, and the one
            // coordinate parser with no local-coordinate branch. `vec2()` is
            // `new Vec2Argument(true)`; `centerCorrect` changes the VALUE a
            // relative coordinate resolves to and not what the parse consumes,
            // so one spec covers both factories.
            new Spec("minecraft:vec2", props(), Vec2Argument.vec2()),
            new Spec("minecraft:component", props(),
                     net.minecraft.commands.arguments.ComponentArgument.textComponent(ctx)),
            new Spec("minecraft:int_range", props(),
                     net.minecraft.commands.arguments.RangeArgument.intRange()),
            new Spec("minecraft:particle", props(),
                     net.minecraft.commands.arguments.ParticleArgument.particle(ctx)),
            new Spec("minecraft:rotation", props(),
                     net.minecraft.commands.arguments.coordinates.RotationArgument.rotation()),
            // The three `/scoreboard` parsers this server reads.
            // `ObjectiveArgument.parse` is `readUnquotedString(); areturn` and
            // therefore has no throwing path at all — see `NEVER_REJECTS` in
            // `ci/command-parse-golden.sh`, where it joins the two
            // `brigadier:string` shapes for exactly that reason.
            new Spec("minecraft:objective", props(), ObjectiveArgument.objective()),
            new Spec("minecraft:objective_criteria", props(),
                     ObjectiveCriteriaArgument.criteria()),
            new Spec("minecraft:scoreboard_slot", props(),
                     ScoreboardSlotArgument.displaySlot()),
            new Spec("minecraft:gamemode", props(), GameModeArgument.gameMode()),
            new Spec("minecraft:resource_location", props(),
                     IdentifierArgument.id()),
            new Spec("minecraft:time", props("min", 0), TimeArgument.time(0)),
            new Spec("minecraft:time", props("min", Integer.MIN_VALUE),
                     TimeArgument.time(Integer.MIN_VALUE)),
            new Spec("minecraft:block_state", props(), BlockStateArgument.block(ctx)),
            new Spec("minecraft:item_stack", props(), ItemArgument.item(ctx)),
        };

        JsonArray cases = new JsonArray();
        int accepted = 0;
        for (Spec spec : specs) {
            for (String input : COMMON) {
                JsonObject row = new JsonObject();
                row.addProperty("parser", spec.parser());
                row.add("props", spec.props());
                row.addProperty("input", input);
                StringReader reader = new StringReader(input);
                try {
                    spec.type().parse(reader);
                    row.addProperty("accepted", true);
                    accepted++;
                } catch (RuntimeException e) {
                    // **A PARSER THAT THROWS AN UNCHECKED EXCEPTION IS NOT A
                    // PARSER THAT REJECTED.** `ObjectiveCriteria.byName` sends
                    // any name containing a `:` to `Identifier.bySeparator`,
                    // which calls `assertValidPath` and throws
                    // `IdentifierException` — a `RuntimeException`, not a
                    // `CommandSyntaxException`. On `stone{Foo:1b}` that
                    // escapes `ObjectiveCriteriaArgument.parse` entirely and
                    // would reach `Commands.executeCommand`'s catch-all as an
                    // internal error rather than a syntax error.
                    //
                    // Recorded rather than swallowed, and marked, because
                    // "vanilla rejected this" and "vanilla blew up on this"
                    // are different facts and a port cannot be graded against
                    // the second. `command_parse_vanilla.rs` requires the port
                    // to answer `Unsupported` on every marked row: claiming a
                    // verdict where vanilla gave none is the failure.
                    row.addProperty("accepted", false);
                    row.addProperty("unchecked", e.getClass().getName());
                    row.addProperty("error", String.valueOf(e));
                    row.addProperty("cursor", reader.getCursor());
                    cases.add(row);
                    continue;
                } catch (CommandSyntaxException e) {
                    row.addProperty("accepted", false);
                    // THE CLASS NAME, AND THE COMMENT ABOVE THIS LINE USED TO
                    // BE FALSE. It said "the type's simple name" and the code
                    // was `String.valueOf(e.getType())`. Brigadier's exception
                    // types override no `toString()`, so that is
                    // `Object.toString()` — a class name plus a **per-JVM
                    // identity hash code**, e.g.
                    // `com.mojang.brigadier.exceptions.DynamicCommandExceptionType@6c5ec68d`.
                    //
                    // It is not merely wrong, it is nondeterministic, and this
                    // dumper's own two-run reproducibility check is what
                    // caught it: `ci/command-parse-golden.sh` compares two
                    // JVM runs with `cmp` and refuses if they differ. On
                    // 2026-09-05 it refused in `~/rustmc-stackgp`, with every
                    // differing line being one of these hashes
                    // (`@2839d77f` against `@6c5ec68d`), and blocked
                    // `stack-gameplay` at table generation before T0 ran.
                    //
                    // It passes far more often than it fails, which is worse
                    // than failing always: HotSpot's default identity hash is
                    // a thread-local xorshift, so it reproduces for an
                    // identical allocation order in the same thread and stops
                    // reproducing the moment anything perturbs one.
                    //
                    // `getClass().getName()` is a compile-time name, stable
                    // across JVMs and across builds, and it is what the old
                    // comment always claimed this field held.
                    //
                    // NOTHING READS IT TODAY. `command_parse_vanilla` compares
                    // `parser`, `input`, `props`, `accepted` and `cursor`, and
                    // not this. It is kept because which *kind* of exception a
                    // parser raised is the obvious next assertion and the
                    // column costs nothing; it is named here as unread so that
                    // its presence is not mistaken for coverage.
                    row.addProperty("error", e.getType().getClass().getName());
                }
                // THE FIELD THIS FILE EXISTS FOR. Recorded on BOTH paths,
                // because a reader that rewound before raising and one that did
                // not are different behaviours and only the cursor separates
                // them.
                //
                // It is a UTF-16 index, which is what `StringReader` counts. A
                // reader indexing UTF-8 bytes has to convert, and the `café`
                // rows are here so that a port which forgets is red rather than
                // subtly wrong on one row in fifty.
                row.addProperty("cursor", reader.getCursor());
                cases.add(row);
            }
        }

        JsonObject doc = new JsonObject();
        doc.addProperty("note",
            "what vanilla's own argument parsers accept, reject, and where they "
            + "leave the cursor. `cursor` is UTF-16 code units, StringReader's own "
            + "index. `error` is diagnostic and is not meant to be compared.");
        doc.addProperty("version", SharedConstants.getCurrentVersion().name());
        doc.addProperty("brigadier", "1.3.10");
        doc.addProperty("specs", specs.length);
        doc.addProperty("inputs", COMMON.length);
        // NOT `cases`: gson's `addProperty` and `add` share a key space, so a
        // count written under the array's own name is silently replaced by the
        // array. It was, until 2026-09-05, and the Rust side's
        // "did the document arrive whole" check had nothing to read.
        doc.addProperty("case_count", cases.size());
        doc.addProperty("accepted", accepted);
        doc.add("cases", cases);

        Path out = Path.of(System.getProperty("rustmc.parse.out"));
        Gson gson = new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create();
        Files.writeString(out, gson.toJson(doc), StandardCharsets.UTF_8);

        PrintStream log = Bootstrap.STDOUT;
        log.println("command-parse-golden: " + specs.length + " specs x " + COMMON.length
            + " inputs = " + cases.size() + " cases, " + accepted + " accepted, "
            + (cases.size() - accepted) + " rejected");
    }
}
