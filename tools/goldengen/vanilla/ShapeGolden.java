// ShapeGolden — `BlockState.updateShape`, for every state that a neighbour
// can destroy.
//
// # What this is for
//
// Nothing cascaded. A torch hung on air because a block a player changed was
// written and broadcast and that was the end of it. Vanilla's cascade is
// `Level.setBlock`, and the half of it that destroys things is
//
//     newState.updateNeighbourShapes(level, pos, flags, recursionLeft)
//       -> for d in UPDATE_SHAPE_ORDER:
//            NeighborUpdater.executeShapeUpdate(level, d.getOpposite(),
//                pos.relative(d), pos, newState, flags, recursionLeft)
//              -> updated = there.updateShape(..., direction, pos, newState, rng)
//              -> Block.updateOrDestroy(there, updated, level, pos.relative(d), ...)
//
// `updateShape` is per block *class*, so it is the same problem
// `getStateForPlacement` was and it gets the same answer: do not port the
// rule, ask vanilla and write down what it said.
//
// # The shape of the question, and why the table is exact rather than sampled
//
// For the destruction family, `updateShape` reads **one** position: the
// neighbour that changed. `BaseTorchBlock` is
//
//     direction == DOWN && !canSurvive(state, level, pos) ? AIR : super
//
// and `canSurvive` is `canSupportCenter(level, pos.below(), UP)`, which is
// `pos.relative(DOWN)` — the neighbour of that very call. `WallTorchBlock`
// guards on `direction.getOpposite() == FACING` and then reads
// `pos.relative(FACING.getOpposite())`, which under that guard is again
// `pos.relative(direction)`. So the answer is a pure function of
//
//     (subject state, direction the change came from, the neighbour's state)
//
// and the third argument ranges over a finite set — every block state in the
// game. This dumps all of it. Not a sample: the level handed to `updateShape`
// answers those two positions and **throws for every other**, so a rule that
// reads more of the world than one neighbour cannot be answered here and is
// recorded as a refusal rather than as a plausible false. Same discipline as
// `SurvivalGolden`, and the same reason: the port must never be asked to
// reproduce a guess.
//
// # Two passes, and since 2026-08-18 both of them are exhaustive
//
//   1. **Discovery.** Every state, every direction, against **every one of
//      the 32,366 block states**, stopping at the first that moves the
//      answer. A pair nothing moves is inert and is not carried.
//   2. **Resolution.** For each pair discovery found, sweep all 32,366
//      neighbour states again and record which ones change the answer.
//
// **Discovery used to sweep the 1,196 block *defaults* and that was a hole,
// not a sampling decision.** The header said so — "a subject that reacts to
// some non-default state of a block and to no block's default state is missed
// by pass 1" — and nobody had put a number on it until `ShapeReadsGolden`
// re-swept the dropped pairs: 90,878 pairs were dropped and **164 of them
// move**. Those 164 had no row, no refusal, and no place in `carried`,
// `connective` or `refusedPairs`. `carried + connective + refused ==
// sensitive` balanced perfectly while 164 real cascades sat outside every
// term in it. 160 of them are the double-chest pairing rule, which reacts to
// a chest that is already `type=left` or `type=right` and to no block's
// default; the other four are `PointedDripstoneBlock` and `SulfurSpikeBlock`.
//
// The cost of closing it is one loop bound. A pair discovery *finds* early
// exits at the same candidate it always did, so the sensitive population pays
// nothing; only the 90,878 that are genuinely inert now read 32,366 states
// instead of 1,196, and that is about 2.9e9 extra probes on a run that
// already does 3.4e9.
//
// **The refusal set barely moves, and the prediction that it would not move
// at all was wrong by two.** `ShapeReadsGolden` re-swept every dropped pair
// with this same narrow probe and reported `discoveryLateRefusal = 0`, and
// that was read as "widening adds 164 to `sensitive` and 0 to
// `refusedPairs`". It measured 32 carried, 130 connective and **2 refused**.
//
// `discoveryLateRefusal = 0` is a statement about **discovery** and not about
// resolution, and the two passes refuse at different moments. Discovery stops
// at the first candidate that moves the answer; resolution then sweeps every
// state from the beginning and can meet a throw that discovery stopped short
// of. A pair discovery never found could not reach resolution at all, so no
// measurement of discovery could have said what resolution would do with it.
// Two of the 164 read past their own neighbour once resolution asks them.
//
// The lesson is not about this table. It is that a count of what one pass
// refuses is not a bound on what a later pass will refuse, and a partition
// re-derived after a breadth change is the only thing that says so.
//
// # What is emitted and what is refused
//
// A pair is emitted only when the sweep produces the subject's own state and
// **one** other. That is the destruction family — `X` or air — and it is what
// a cascade needs. A pair with several alternatives is a *connection*
// property — a stair choosing an inner or outer corner, a rail its curve, a
// chest its half — which changes how a block looks and never whether it
// exists. Those are counted in `connective` and not carried, which is the
// same gap `placement.json` already names as its 195 neighbour-sensitive
// blocks: the two are the same machinery seen from opposite sides.
//
// **A FENCE IS NOT ONE OF THEM, AND THIS HEADER SAID IT WAS.** The example
// above read "a fence deciding which way it joins" until 2026-08-18, and it
// was wrong by this paragraph's own rule: a fence's join is one boolean per
// direction, so a (state, direction) pair answers with the block's own state
// and exactly one other — the emission rule, not the refusal rule. This table
// carries **all 128 `oak_fence` pairs** and **zero `oak_stairs` pairs**, which
// is why `140-fences-connect-to-each-other` matches and
// `180-stairs-make-a-corner-when-two-meet` diverges.
//
// It cost more than a wrong word. A static audit of the 22 diverging checks
// quoted it, grouped four diverging connection checks with the passing fence
// one, and could not explain why the fence passed — because there is no
// fence-specific code to find and the comment said the generic path could not
// do it either. `crate::shapes` corrected its own copy on 2026-08-04 and
// deliberately left this one, because this header's digest is part of
// `update_shape.json`'s provenance stamp (`ci/provenance.sh`, `gen_md5`) and
// touching it marks the table STALE until it is regenerated. Its note said to
// fix it in the same commit as the next regeneration. This is that commit.
//
// # 79,113 refused is not 79,113 owed
//
// The refusal count is the largest number this dumper produces and it is not
// a measure of debt, because the discovery loop above **breaks on the first
// throw** — a pair is recorded as refused before anything establishes that its
// answer ever moves. `ShapeReadsGolden` re-asks every refused pair against a
// level that answers past the neighbour, and of the 79,113: 48,081 need a real
// world, 22,010 are a single-alternative destruction rule invariant under air,
// stone and water, 5,609 are the connection family counted a second time,
// 3,089 are inert over all 32,366 neighbour states, and 324 survive widening
// on `getLightEngine`, `environmentAttributes` and `registryAccess`. The debt
// is 48,405, and `WallBlock` alone is 41,472 of it.
//
// 78,797 of the refusals name a **position** rather than a level method, and
// 54,724 of those read exactly one position past their own neighbour.
// `tools/goldengen/vanilla/ShapeThreeGolden.java` is the extract that answers
// three positions and throws for a fourth. The path is written in BACKTICKS
// here and must stay that way: `ci/gen-gameplay-tables.sh` derives its required
// set by grepping this directory for a **quoted** `"data/vanilla/<name>.json"`,
// and it does not distinguish a comment from a call. Put double quotes around
// the path in this sentence and every tree on the box starts paying twenty
// minutes at setup to generate a table nothing opens. `data/vanilla/update_shape3.json`
// is what it produces, and unlike the census it invents nothing.
//
// # The 164 that were in no count at all
//
// Closed here rather than named and left. See the two-pass section above: the
// number came out of `ShapeReadsGolden`, the fix is discovery's loop bound,
// and what it buys is that `carried + connective + refused == sensitive` is
// now *exhaustive* and not merely balanced. A partition that adds up while
// real rows sit outside every term in it is the failure mode this project
// calls unfalsifiable, and it was in the oldest and largest table here.
//
// The neighbour sets are bitmaps over the global block-state id, deduplicated
// across the whole table — a torch, a rail and a sapling that agree about
// what can hold them share one entry. Base64 of 4,046 bytes each.
//
// Converted to JSON by `ci/shape-golden.sh`, which runs it twice and demands
// the two agree.

package net.minecraft.world.level.levelgen;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.stream.IntStream;

import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.LevelReader;
import net.minecraft.world.level.ScheduledTickAccess;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;

public final class ShapeGolden {

    /// Where the subject sits. Any y inside a normal world does; the level
    /// below reports `-64` and `384` so a rule that checks build height gets
    /// the real bounds rather than a corner case.
    private static final BlockPos POS = new BlockPos(0, 64, 0);

    /// `Direction.values()` — the order `block_props.json` writes its face
    /// bitstrings in and the order the port indexes by.
    private static final Direction[] DIRECTIONS = Direction.values();

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        SurfaceChunkGolden.bindBlockTags();
        SurfaceChunkGolden.bindFluidTags();

        List<BlockState> states = new ArrayList<>();
        for (BlockState state : Block.BLOCK_STATE_REGISTRY) {
            states.add(state);
        }
        int n = states.size();
        // `Block.getId` is the id `block_props.json` carries and the id the
        // wire uses. Read rather than assumed to be the iteration order.
        BlockState[] byId = new BlockState[n];
        for (BlockState state : states) {
            byId[Block.getId(state)] = state;
        }
        for (int i = 0; i < n; i++) {
            if (byId[i] == null) {
                throw new IllegalStateException("state id " + i + " is not in the registry");
            }
        }

        // One row per (state, direction) that anything can change, resolved
        // over every neighbour state. Parallel over subjects and collected in
        // id order, so the file does not depend on how the work was split.
        Row[][] rows = new Row[n][];
        IntStream.range(0, n).parallel().forEach(id -> rows[id] = resolve(byId[id], byId));

        // Deduplicate the neighbour sets. A torch, a rail and a sapling that
        // agree about what can hold them are one entry.
        Map<String, Integer> classes = new LinkedHashMap<>();
        List<String> table = new ArrayList<>();
        // Refusals are summarised rather than listed. There are tens of
        // thousands and the port cannot act on any of them individually — it
        // is the *reason* that says what is missing, and a per-pair list made
        // the file 16 MB, which every consumer then had to parse.
        Map<String, java.util.TreeSet<String>> refusedBlocks = new java.util.TreeMap<>();
        Map<String, Integer> refusedCounts = new java.util.TreeMap<>();
        int refusedPairs = 0;
        int connective = 0;
        int sensitive = 0;
        for (int id = 0; id < n; id++) {
            for (Row row : rows[id]) {
                sensitive++;
                if (row.refusal != null) {
                    refusedPairs++;
                    refusedCounts.merge(row.refusal, 1, Integer::sum);
                    // EVERY block, not the first eight. This was
                    // `if (blocks.size() < 8)`, which made `blocks` a sample
                    // and made its length look like a count: six of the eleven
                    // refusal reasons reported exactly 8 blocks, and a reader
                    // taking the union got "37 distinct blocks refused" for a
                    // number that was a truncation. The set is a TreeSet of
                    // block names — tens of entries against a 3.4 MB document
                    // — and it is the only thing in this file a port can act
                    // on, because it is the list of what has to be written.
                    // The per-*pair* list is still summarised, for the reason
                    // above: 16 MB nobody could use.
                    refusedBlocks
                        .computeIfAbsent(row.refusal, k -> new java.util.TreeSet<>())
                        .add(row.state.split("\\[")[0]);
                    continue;
                }
                if (row.to == null) {
                    connective++;
                    continue;
                }
                Integer index = classes.get(row.set);
                if (index == null) {
                    index = classes.size();
                    classes.put(row.set, index);
                }
                table.add("{\"state\": " + quote(row.state)
                    + ", \"dir\": " + row.dir
                    + ", \"to\": " + quote(row.to)
                    + ", \"schedules\": " + row.schedules
                    + ", \"class\": " + index + "}");
            }
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"update_shape\",\n");
        out.append("  \"states\": ").append(n).append(",\n");
        out.append("  \"directionOrder\": [");
        for (int i = 0; i < DIRECTIONS.length; i++) {
            out.append(i > 0 ? ", " : "").append(quote(DIRECTIONS[i].name()));
        }
        out.append("],\n");
        out.append("  \"sensitive\": ").append(sensitive).append(",\n");
        out.append("  \"connective\": ").append(connective).append(",\n");
        out.append("  \"refusedPairs\": ").append(refusedPairs).append(",\n");
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
                .append(", \"pairs\": ").append(entry.getValue())
                .append(", \"blockCount\": ").append(blocks.size())
                .append(", \"blocks\": [").append(names).append("]}")
                .append(seen < refusedCounts.size() ? ",\n" : "\n");
        }
        out.append("  ],\n  \"classes\": [\n");
        int written = 0;
        for (String key : classes.keySet()) {
            written++;
            out.append("    ").append(quote(key)).append(written < classes.size() ? ",\n" : "\n");
        }
        out.append("  ],\n  \"table\": [\n");
        for (int i = 0; i < table.size(); i++) {
            out.append("    ").append(table.get(i)).append(i + 1 < table.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    /// One (state, direction) pair the world can do something to.
    private static final class Row {
        private final String state;
        private final int dir;
        /// The one other answer the sweep produced, or `null` when there were
        /// several — a connection property rather than a destruction.
        private String to;
        /// Base64 of the neighbour-state bitmap that produces [`to`].
        private String set;
        /// What the rule read that this harness cannot answer.
        private String refusal;
        /// Whether resolving this pair ever asked for a scheduled tick.
        /// Carried because it is the *next* thing the server needs and this
        /// sweep is the only place that already knows.
        private boolean schedules;

        private Row(String state, int dir) {
            this.state = state;
            this.dir = dir;
        }
    }

    /// Discovery then resolution, for one subject state.
    private static Row[] resolve(BlockState subject, BlockState[] byId) {
        String name = BlockStateParser.serialize(subject);
        List<Row> rows = new ArrayList<>();
        for (int d = 0; d < DIRECTIONS.length; d++) {
            Probe probe = new Probe(subject, DIRECTIONS[d]);
            // Pass 1: is this pair inert? Against **every** block state,
            // which answers "no" for almost every one of the 194,196 pairs
            // and stops at the first candidate that says otherwise.
            //
            // This read the 1,196 block defaults until 2026-08-18 and dropped
            // 164 pairs that only a non-default neighbour moves — see the
            // header. The early exit is why the widening is nearly free: a
            // pair that moves at all still stops at the same candidate.
            boolean moved = false;
            String refusal = null;
            for (BlockState candidate : byId) {
                try {
                    if (probe.answer(candidate) != subject) {
                        moved = true;
                        break;
                    }
                } catch (RuntimeException e) {
                    refusal = message(e);
                    break;
                }
            }
            if (refusal == null && !moved) {
                continue;
            }
            Row row = new Row(name, d);
            if (refusal != null) {
                row.refusal = refusal;
                rows.add(row);
                continue;
            }
            // Pass 2: every state in the game, exhaustively.
            byte[] bits = new byte[(byId.length + 7) / 8];
            BlockState other = null;
            boolean several = false;
            for (int id = 0; id < byId.length; id++) {
                BlockState answer;
                try {
                    answer = probe.answer(byId[id]);
                } catch (RuntimeException e) {
                    row.refusal = message(e);
                    break;
                }
                if (answer == subject) {
                    continue;
                }
                if (other == null) {
                    other = answer;
                } else if (answer != other) {
                    several = true;
                    break;
                }
                bits[id >> 3] |= (byte) (1 << (id & 7));
            }
            if (row.refusal == null && !several && other != null) {
                row.to = BlockStateParser.serialize(other);
                row.set = Base64.getEncoder().encodeToString(bits);
                row.schedules = probe.scheduled;
            }
            rows.add(row);
        }
        return rows.toArray(new Row[0]);
    }

    private static String message(RuntimeException e) {
        String m = e.getMessage();
        return m == null ? e.getClass().getName() : m;
    }

    /// The subject, one neighbour, and a level that knows nothing else.
    ///
    /// The proxy answers `getBlockState` and `getFluidState` for exactly two
    /// positions and **throws** for a third, so a rule that walks further
    /// than its own neighbour is refused rather than answered from a
    /// plausible air. Everything else the interfaces declare is either a
    /// default method — routed to its own implementation with
    /// `InvocationHandler.invokeDefault`, so `scheduleTick` behaves as
    /// vanilla wrote it — or a world fact with one honest answer.
    private static final class Probe {
        private final BlockState subject;
        private final Direction direction;
        private final BlockPos neighborPos;
        private final Object level;
        private final RandomSource random = RandomSource.create(0L);
        private BlockState neighbor;
        /// Set when a rule asked to be woken later — see [`Row#schedules`].
        private boolean scheduled;

        private Probe(BlockState subject, Direction direction) {
            this.subject = subject;
            this.direction = direction;
            this.neighborPos = POS.relative(direction);
            InvocationHandler handler = (proxy, method, args) -> {
                String call = method.getName();
                if (call.equals("getBlockState") && method.getParameterCount() == 1) {
                    return stateAt((BlockPos) args[0]);
                }
                if (call.equals("getFluidState") && method.getParameterCount() == 1) {
                    return stateAt((BlockPos) args[0]).getFluidState();
                }
                if (call.equals("getBlockEntity") && method.getParameterCount() == 1) {
                    return blockEntity((BlockPos) args[0]);
                }
                if (call.equals("getMinY")) {
                    return -64;
                }
                if (call.equals("getHeight") && method.getParameterCount() == 0) {
                    return 384;
                }
                if (call.equals("getSeaLevel")) {
                    return 63;
                }
                if (call.equals("isClientSide")) {
                    return false;
                }
                if (call.equals("getRandom")) {
                    return random;
                }
                if (call.equals("toString")) {
                    return "one-neighbour level";
                }
                if (call.equals("hashCode")) {
                    return System.identityHashCode(proxy);
                }
                if (call.equals("equals")) {
                    return proxy == args[0];
                }
                // A scheduled tick is a *sink*, not an answer: five blocks
                // schedule one from `getStateForPlacement` and the coral
                // family schedules one here. Accepting it costs nothing and
                // refusing it would refuse the whole rule.
                if (call.equals("getBlockTicks") || call.equals("getFluidTicks")) {
                    scheduled = true;
                    return ticks();
                }
                if (call.equals("createTick")) {
                    scheduled = true;
                    return null;
                }
                if (method.isDefault()) {
                    return InvocationHandler.invokeDefault(proxy, method, args);
                }
                throw new UnsupportedOperationException(
                    "updateShape read " + method.getDeclaringClass().getSimpleName() + "." + call);
            };
            this.level = Proxy.newProxyInstance(
                ShapeGolden.class.getClassLoader(),
                new Class<?>[]{LevelReader.class, ScheduledTickAccess.class},
                handler);
        }

        private BlockState stateAt(BlockPos pos) {
            if (pos.equals(POS)) {
                return subject;
            }
            if (pos.equals(neighborPos)) {
                return neighbor;
            }
            throw new UnsupportedOperationException(
                "updateShape read " + relative(pos) + ", not its own neighbour");
        }

        /// The block entity a freshly placed block of that state would have.
        ///
        /// Answering `null` here is not neutral — it is a shulker box with no
        /// shulker box in it, and `ShulkerBoxBlock.getBlockSupportShape`
        /// reads exactly that to decide whether its face is solid. Refusing
        /// instead lost 10,563 pairs to a candidate *ground* rather than to
        /// anything about the subject, which is the oracle answering a
        /// question about itself. `EntityBlock.newBlockEntity` is what the
        /// world holds the moment the block is placed, so that is what this
        /// hands back.
        private Object blockEntity(BlockPos pos) {
            BlockState state = stateAt(pos);
            if (state.getBlock() instanceof net.minecraft.world.level.block.EntityBlock entity) {
                return entity.newBlockEntity(pos, state);
            }
            return null;
        }

        /// A position named by where it is relative to the subject, so a
        /// refusal says *what* the rule wanted rather than an absolute
        /// coordinate that means nothing outside this harness.
        private static String relative(BlockPos pos) {
            return (pos.getX() - POS.getX()) + "," + (pos.getY() - POS.getY())
                + "," + (pos.getZ() - POS.getZ());
        }

        private Object ticks() {
            return Proxy.newProxyInstance(
                ShapeGolden.class.getClassLoader(),
                new Class<?>[]{net.minecraft.world.ticks.LevelTickAccess.class},
                (proxy, method, args) -> {
                    switch (method.getName()) {
                        case "hasScheduledTick", "willTickThisTick" -> {
                            return false;
                        }
                        case "count" -> {
                            return 0;
                        }
                        case "toString" -> {
                            return "sink";
                        }
                        case "hashCode" -> {
                            return System.identityHashCode(proxy);
                        }
                        case "equals" -> {
                            return proxy == args[0];
                        }
                        default -> {
                            Class<?> ret = method.getReturnType();
                            if (ret == boolean.class) {
                                return false;
                            }
                            if (ret == int.class) {
                                return 0;
                            }
                            return null;
                        }
                    }
                });
        }

        private BlockState answer(BlockState candidate) {
            neighbor = candidate;
            return subject.updateShape(
                (LevelReader) level,
                (ScheduledTickAccess) level,
                POS,
                direction,
                neighborPos,
                candidate,
                random);
        }
    }

    private static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private ShapeGolden() {}
}
