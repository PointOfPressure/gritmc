// PistonGolden — the three per-state facts `PistonBaseBlock.isPushable` reads
// and this server had no table for.
//
// `isPushable` asks four questions of a block. Three of them are answered by
// data that exists nowhere else in `data/vanilla`:
//
//   * `state.getPistonPushReaction()` — NORMAL / DESTROY / BLOCK / PUSH_ONLY;
//   * `state.getDestroySpeed(level, pos) == -1.0F` — unbreakable;
//   * `state.hasBlockEntity()`.
//
// The fourth, the four named immovable blocks, is a literal in the bytecode
// and is a literal in our port too.
//
// **Why a table rather than a default.** A port that defaulted the reaction to
// NORMAL would push every unlisted block, which is right for most of the
// registry and silently wrong for the ones that matter — and wrong in the
// direction that produces a working-looking piston. That is the check-that-
// cannot-go-red class this repo keeps paying for: the failure would be
// invisible in every scenario whose blocks happen to be NORMAL, which is
// every scenario anybody writes first.
//
// Emitted per **block** where all of that block's states agree, which is
// almost all of them, and per **state** where they do not, so the file stays
// small without inventing a uniformity vanilla does not have.
//
// Grep any diagnostic output unanchored — `Bootstrap.bootStrap()` redirects
// System.out through log4j and prefixes every line.
package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;

public final class PistonGolden {
    private PistonGolden() {}

    /// One state's three answers, as the comma-joined string used for the
    /// all-states-agree test.
    private static String row(BlockState state) {
        String reaction = state.getPistonPushReaction().name();
        boolean unbreakable =
                state.getDestroySpeed(EmptyBlockGetter.INSTANCE, BlockPos.ZERO) == -1.0F;
        return reaction + "," + unbreakable + "," + state.hasBlockEntity();
    }

    private static String json(String block, String row) {
        String[] p = row.split(",");
        return "\"" + block + "\": {\"reaction\": \"" + p[0] + "\", \"unbreakable\": " + p[1]
                + ", \"blockEntity\": " + p[2] + "}";
    }

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // Sorted by name. `BuiltInRegistries.BLOCK` iterates in registration
        // order, which is stable, but the two-run comparison in the shell
        // wrapper is worth nothing if anything downstream ever is not.
        Map<String, String> uniform = new TreeMap<>();
        Map<String, String> perState = new TreeMap<>();
        int blocks = 0;
        int split = 0;

        Map<Block, List<BlockState>> byBlock = new LinkedHashMap<>();
        for (BlockState state : Block.BLOCK_STATE_REGISTRY) {
            byBlock.computeIfAbsent(state.getBlock(), b -> new ArrayList<>()).add(state);
        }

        for (Map.Entry<Block, List<BlockState>> e : byBlock.entrySet()) {
            String name = BuiltInRegistries.BLOCK.getKey(e.getKey()).toString();
            blocks++;
            String first = row(e.getValue().get(0));
            boolean agree = true;
            for (BlockState s : e.getValue()) {
                if (!row(s).equals(first)) {
                    agree = false;
                    break;
                }
            }
            if (agree) {
                uniform.put(name, first);
            } else {
                split++;
                for (BlockState s : e.getValue()) {
                    perState.put(
                            net.minecraft.commands.arguments.blocks.BlockStateParser
                                    .serialize(s),
                            row(s));
                }
            }
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n  \"_\": \"piston_push\",\n");
        out.append("  \"blocks\": {\n");
        boolean firstRow = true;
        for (Map.Entry<String, String> e : uniform.entrySet()) {
            if (!firstRow) {
                out.append(",\n");
            }
            firstRow = false;
            out.append("    ").append(json(e.getKey(), e.getValue()));
        }
        out.append("\n  },\n  \"states\": {\n");
        firstRow = true;
        for (Map.Entry<String, String> e : perState.entrySet()) {
            if (!firstRow) {
                out.append(",\n");
            }
            firstRow = false;
            out.append("    ").append(json(e.getKey(), e.getValue()));
        }
        out.append("\n  }\n}\n");
        // **Nothing but the JSON may be printed, on either stream.**
        // `Bootstrap.bootStrap()` reroutes System.err through log4j, whose
        // appender writes to **stdout** with a `[HH:MM:SS] [main/INFO]:
        // [STDERR]: ` prefix — so a summary line written to stderr lands
        // inside the JSON file, carrying a timestamp that differs between
        // runs. That is what the wrapper's two-run comparison caught the
        // first time this was run, and it would otherwise have shipped a
        // file with a trailing garbage line that no JSON parser accepts.
        //
        // The counts the summary carried are recovered by the wrapper from
        // the file itself, which is the only place they can be checked
        // against what was actually written.
        System.out.print(out);
        assert blocks >= 0 && split >= 0;
    }
}
