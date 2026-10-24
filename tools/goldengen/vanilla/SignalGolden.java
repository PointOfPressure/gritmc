// SignalGolden — `BlockState.getSignal` and `getDirectSignal`, for every state.
//
// # What this is for
//
// `Level.setBlock`'s *other* half. `updateNeighbourShapes` asks a neighbour
// what it becomes; `updateNeighborsAt` tells a neighbour that something
// changed, and everything the neighbour then does is a function of how much
// redstone signal it can see. The whole of that question funnels through two
// methods on `BlockState`:
//
//     SignalGetter.getSignal(pos, dir)       = getBlockState(pos).getSignal(this, pos, dir)
//     SignalGetter.getDirectSignal(pos, dir) = getBlockState(pos).getDirectSignal(this, pos, dir)
//
// and `getBestNeighborSignal`, `getDirectSignalTo`, `hasNeighborSignal` and
// `hasSignal` are all written in terms of those two. So a table of the two,
// indexed by state and direction, answers the level-side rule outright and the
// port is left with only the blocks whose answer is not a function of their own
// state.
//
// # Why a table and not twenty hand ports
//
// The signal surface is small enough to hand port and large enough that doing
// so would be twenty separate chances to write a silent zero. `BlockBehaviour`
// defaults all three of `isSignalSource`, `ownSignal` and `getDirectSignal` to
// the falsy answer, so **a block nobody ported is indistinguishable from a
// block that emits nothing** — which is this project's oldest failure mode and
// the exact shape that made `update_shape.json`'s 1,296 all-`DOWN` wire rows
// read as a complete table. A measured table cannot have that defect: a state
// this harness could not answer is a *refusal* with a reason attached, and a
// refusal is loud.
//
// Twenty is not a guess. Grepping the constant pool of every class under
// `net/minecraft/world/level/block/` for the three method names returns
// `ownSignal` in 20 classes, `getDirectSignal` in 13 and `getSignal` in 11,
// against `BlockBehaviour`'s three defaults:
//
//     isSignalSource   -> false
//     getSignal        -> ownSignal(state, level, pos)
//     ownSignal        -> 0
//     getDirectSignal  -> 0
//
// # What is refused, and why the refusal is per STATE and not per direction
//
// The level handed to the two calls answers `getBlockState` for **one**
// position — the subject's own — and throws for everything else, including
// every request for a block entity. A rule that reads further than itself
// therefore cannot be answered here.
//
// The probe stops at the first throw and marks the **whole state** refused,
// rather than keeping the directions that happened to answer before it. That
// is deliberate and it is the lesson `update_shape.json` taught at cost: a
// partial row reads exactly like a complete one from the call site, and a
// port that found five answered directions and one refusal would have no way
// to notice it was holding five sixths of a rule. A block whose rule reads the
// world is a hand port for all six directions or for none.
//
// **`getBlockEntity` is a refusal here and is an answer in `ShapeGolden`.**
// The two are not inconsistent. `ShapeGolden` needs a block entity because
// `ShulkerBoxBlock` asks one about its *shape*, and a freshly placed shulker
// box has the shape a freshly placed shulker box has. Here the block entity
// **is the value** — a comparator's output signal is a field on
// `ComparatorBlockEntity` and a fresh one holds 0 — so answering would record
// "every comparator in the game outputs nothing" as a measurement.
//
// # `shouldSignal`, the mutable field, and what this table therefore means
//
// `RedStoneWireBlock` has a `boolean shouldSignal` **field**, initialised
// `true` in the constructor and set `false` for the duration of
// `getBlockSignal`. While it is false the wire's `getSignal`, `getDirectSignal`
// and `isSignalSource` all answer the falsy value, which is how a wire is kept
// from counting other wires as sources while it works out its own strength.
//
// This harness only ever sees `shouldSignal == true`, because nothing here
// calls `getBlockSignal`. So every wire row in this table is the
// **should-signal-true** answer, and the port has to carry the field itself.
// It is written down here because a table that silently recorded one of two
// modes would be wrong exactly half the time in the one place it is read most.
//
// # What is emitted
//
//   state    `BlockStateParser.serialize` — the key, and the id beside it.
//   source   `BlockState.isSignalSource()`. Duplicated from
//            `block_props.json` on purpose: two independently generated
//            columns that must agree is a gate, and one column is not.
//   signal   six values, `getSignal` in `Direction.values()` order —
//            DOWN, UP, NORTH, SOUTH, WEST, EAST, the same order
//            `block_props.json` writes its face bits in.
//   direct   six values, `getDirectSignal`, same order.
//
// A state with no signal, no direct signal and `source == false` is the
// overwhelming majority and is not carried: it is what the port's default
// already is. `states` and `quiet` are emitted so the arithmetic can be
// checked — carried + refused + quiet must equal states.
//
// Converted to JSON by `ci/signal-golden.sh`, which runs it twice and demands
// the two agree.

package net.minecraft.world.level.levelgen;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.TreeSet;

import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;

public final class SignalGolden {

    /// Where the subject sits. Same position `ShapeGolden` uses, for the same
    /// reason: a y inside a normal world, with the level reporting the real
    /// build bounds so a height test gets a truthful answer.
    private static final BlockPos POS = new BlockPos(0, 64, 0);

    /// `Direction.values()` — DOWN, UP, NORTH, SOUTH, WEST, EAST.
    private static final Direction[] DIRECTIONS = Direction.values();

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        SurfaceChunkGolden.bindBlockTags();
        SurfaceChunkGolden.bindFluidTags();

        int n = 0;
        for (BlockState ignored : Block.BLOCK_STATE_REGISTRY) {
            n++;
        }
        BlockState[] byId = new BlockState[n];
        for (BlockState state : Block.BLOCK_STATE_REGISTRY) {
            byId[Block.getId(state)] = state;
        }
        for (int i = 0; i < n; i++) {
            if (byId[i] == null) {
                throw new IllegalStateException("state id " + i + " is not in the registry");
            }
        }

        Probe probe = new Probe();
        List<String> table = new ArrayList<>();
        Map<String, Integer> refusedCounts = new TreeMap<>();
        Map<String, TreeSet<String>> refusedBlocks = new TreeMap<>();
        List<String> refusedStates = new ArrayList<>();
        int quiet = 0;
        int sources = 0;

        for (int id = 0; id < n; id++) {
            BlockState state = byId[id];
            probe.subject = state;
            int[] signal = new int[DIRECTIONS.length];
            int[] direct = new int[DIRECTIONS.length];
            String refusal = null;
            for (int d = 0; d < DIRECTIONS.length && refusal == null; d++) {
                try {
                    signal[d] = state.getSignal(probe.level, POS, DIRECTIONS[d]);
                    direct[d] = state.getDirectSignal(probe.level, POS, DIRECTIONS[d]);
                } catch (RuntimeException e) {
                    refusal = message(e);
                }
            }
            boolean source;
            try {
                source = state.isSignalSource();
            } catch (RuntimeException e) {
                source = false;
                if (refusal == null) {
                    refusal = message(e);
                }
            }
            String name = BlockStateParser.serialize(state);
            if (refusal != null) {
                refusedCounts.merge(refusal, 1, Integer::sum);
                // EVERY block, not the first eight. The cap this replaces is
                // the one `ShapeGolden` had, where it silently turned a count
                // into a sample: six of `update_shape.json`'s eleven refusal
                // reasons reported exactly 8 blocks and the union read 37 for
                // a true 258. It was never binding here — the two reasons this
                // dumper finds hold 1 and 4 blocks — and that is exactly why
                // it is worth removing: a latent truncation gives no sign on
                // the day it starts lying.
                refusedBlocks
                    .computeIfAbsent(refusal, k -> new TreeSet<>())
                    .add(name.split("\\[")[0]);
                refusedStates.add("    {\"state\": " + quote(name)
                    + ", \"id\": " + id
                    + ", \"source\": " + source
                    + ", \"reads\": " + quote(refusal) + "}");
                continue;
            }
            if (source) {
                sources++;
            }
            if (!source && allZero(signal) && allZero(direct)) {
                quiet++;
                continue;
            }
            table.add("    {\"state\": " + quote(name)
                + ", \"id\": " + id
                + ", \"source\": " + source
                + ", \"signal\": " + ints(signal)
                + ", \"direct\": " + ints(direct) + "}");
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"signal\",\n");
        out.append("  \"states\": ").append(n).append(",\n");
        out.append("  \"directionOrder\": [");
        for (int i = 0; i < DIRECTIONS.length; i++) {
            out.append(i > 0 ? ", " : "").append(quote(DIRECTIONS[i].name()));
        }
        out.append("],\n");
        out.append("  \"quiet\": ").append(quiet).append(",\n");
        out.append("  \"sources\": ").append(sources).append(",\n");
        out.append("  \"refusedStates\": ").append(refusedStates.size()).append(",\n");
        out.append("  \"refused\": [\n");
        int seen = 0;
        for (Map.Entry<String, Integer> entry : refusedCounts.entrySet()) {
            seen++;
            List<String> blocks = new ArrayList<>(refusedBlocks.get(entry.getKey()));
            StringBuilder names = new StringBuilder();
            for (int i = 0; i < blocks.size(); i++) {
                names.append(i > 0 ? ", " : "").append(quote(blocks.get(i)));
            }
            out.append("    {\"reads\": ").append(quote(entry.getKey()))
                .append(", \"states\": ").append(entry.getValue())
                .append(", \"blockCount\": ").append(blocks.size())
                .append(", \"blocks\": [").append(names).append("]}")
                .append(seen < refusedCounts.size() ? ",\n" : "\n");
        }
        out.append("  ],\n  \"unanswerable\": [\n");
        for (int i = 0; i < refusedStates.size(); i++) {
            out.append(refusedStates.get(i)).append(i + 1 < refusedStates.size() ? ",\n" : "\n");
        }
        out.append("  ],\n  \"table\": [\n");
        for (int i = 0; i < table.size(); i++) {
            out.append(table.get(i)).append(i + 1 < table.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    private static boolean allZero(int[] values) {
        for (int v : values) {
            if (v != 0) {
                return false;
            }
        }
        return true;
    }

    private static String ints(int[] values) {
        StringBuilder b = new StringBuilder("[");
        for (int i = 0; i < values.length; i++) {
            b.append(i > 0 ? ", " : "").append(values[i]);
        }
        return b.append("]").toString();
    }

    private static String message(RuntimeException e) {
        String m = e.getMessage();
        return m == null ? e.getClass().getName() : m;
    }

    /// The subject and a level that knows nothing else at all.
    ///
    /// One instance, reused across states — the subject is a mutable field
    /// because the proxy closes over `this` and building 32,366 proxies to
    /// change one reference would be the only slow thing in the harness.
    private static final class Probe {
        private BlockState subject;
        private final BlockGetter level;

        private Probe() {
            InvocationHandler handler = (proxy, method, args) -> {
                String call = method.getName();
                if (call.equals("getBlockState") && method.getParameterCount() == 1) {
                    return stateAt((BlockPos) args[0]);
                }
                if (call.equals("getFluidState") && method.getParameterCount() == 1) {
                    return stateAt((BlockPos) args[0]).getFluidState();
                }
                // Refused rather than answered — see the header. A block
                // entity is the signal, not the shape of the signal.
                if (call.equals("getBlockEntity")) {
                    throw new UnsupportedOperationException(
                        "getSignal read a block entity, which holds the value it is being asked for");
                }
                if (call.equals("getMinY")) {
                    return -64;
                }
                if (call.equals("getHeight") && method.getParameterCount() == 0) {
                    return 384;
                }
                if (call.equals("toString")) {
                    return "one-state level";
                }
                if (call.equals("hashCode")) {
                    return System.identityHashCode(proxy);
                }
                if (call.equals("equals")) {
                    return proxy == args[0];
                }
                if (method.isDefault()) {
                    return InvocationHandler.invokeDefault(proxy, method, args);
                }
                throw new UnsupportedOperationException(
                    "getSignal read " + method.getDeclaringClass().getSimpleName() + "." + call);
            };
            this.level = (BlockGetter) Proxy.newProxyInstance(
                SignalGolden.class.getClassLoader(),
                new Class<?>[]{BlockGetter.class},
                handler);
        }

        private BlockState stateAt(BlockPos pos) {
            if (pos.equals(POS)) {
                return subject;
            }
            throw new UnsupportedOperationException(
                "getSignal read " + relative(pos) + ", not its own position");
        }

        /// Named by offset, so a refusal says what the rule wanted rather than
        /// an absolute coordinate that means nothing outside this harness.
        private static String relative(BlockPos pos) {
            return (pos.getX() - POS.getX()) + "," + (pos.getY() - POS.getY())
                + "," + (pos.getZ() - POS.getZ());
        }
    }

    private static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private SignalGolden() {}
}
