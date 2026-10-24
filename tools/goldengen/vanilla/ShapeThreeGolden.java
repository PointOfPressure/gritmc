// ShapeThreeGolden — the three-position extract: `updateShape` for the pairs
// `ShapeGolden` refuses because the rule reads **one** position past its own
// neighbour.
//
// # What this is and what it is not
//
// `update_shape.json` refuses 79,113 of its 103,482 sensitive (state,
// direction) pairs, and `ci/checks/shape-refusal-census.json` took that number
// apart: 48,405 of them are debt and 54,724 of them read **exactly one**
// position beyond the neighbour that changed. The census could say that much
// and no more, because the level it re-asked with **invents** the world — it
// answers a filler block for every position it was not given, and a row
// derived from an invented world is the plausible answer this project refuses.
//
// This is not that. The level here answers **three** positions — the subject,
// the neighbour that changed, and the one further position the rule was
// measured to read — and **throws for a fourth**, exactly as `ShapeGolden`
// throws for a third. Nothing is invented. A pair whose rule reaches further
// than three positions is refused here too, and says which position it wanted.
//
// The third position is not chosen. It is read out of `ShapeGolden`'s own
// refusal: the narrow probe records the offset it was asked for and threw on,
// and that offset — not a guess about which block a wall cares about — is what
// the extract widens to.
//
// # The cross product, and the two breadths that make it affordable
//
// The answer is now a function of two block states, not one: the neighbour and
// the third position. Sweeping both exhaustively is 32,366 x 32,366 per pair
// and is not a thing anyone can run. So the two axes are swept at different
// breadths, and **both are stated in the output**:
//
//   * the **neighbour** axis is exhaustive — all 32,366 states — because that
//     is the axis `update_shape.json` is keyed on and a bitmap over it is what
//     a consumer reads.
//   * the **third position** axis is the 1,196 block defaults, which is
//     `ShapeGolden`'s own discovery breadth. A third-position state that is
//     not any block's default is not covered, and `extraBreadth` says so.
//
// The saving is that the third axis does not need an exhaustive neighbour
// sweep per value. Two third-position states that produce the same answer
// vector over the 1,196 default neighbours are one **extra class**, and only
// one representative of each class pays for the exhaustive sweep. A wall has
// two (the block above either requires a tall pillar or it does not); most
// pairs have one. The class is derived from the full answer vector and
// compared by content, never by a hash alone — see `classOf`.
//
// So the per-pair cost is 1,196 x 1,196 to partition the third axis, plus one
// exhaustive 32,366 sweep per class. Not 32,366 x 32,366.
//
// # What is emitted
//
// One row per (state, direction, extra class): the offset the rule read, the
// set of third-position blocks that behave that way, the one other state the
// subject becomes, and the neighbour bitmap that produces it — the same shape
// `update_shape.json` carries, with one more key. A class that produces
// several alternatives is `connective`, as it is there. A class that produces
// none is inert.
//
// A pair is **closed** only when every one of its extra classes produced at
// most one alternative. Partial closure is not emitted: half a rule is worse
// than none, because a consumer cannot tell which half it has.
//
// # The refusals this still has, and why they are honest
//
//   * **a fourth position.** The rule read past the third. Named with the
//     offset, so the next extract knows what it would have to widen to.
//   * **a level method.** `getLightEngine`, `environmentAttributes`,
//     `registryAccess` — no widening of a position-answering harness reaches
//     these, and the census already said so.
//   * **too many extra classes.** A cap, stated in the output as a number
//     rather than left implicit, because the cost of a class is an exhaustive
//     sweep and a pair with hundreds of them would run for hours on its own.
//     A pair that hits the cap is refused, not truncated.
//
// # Grep any diagnostic output unanchored
//
// `Bootstrap.bootStrap()` redirects both streams through log4j, which re-emits
// each line with a `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix. This writes its
// JSON to stdout and `ci/shape-three.sh` captures it with `>` rather than
// through a pipe for that reason.

package net.minecraft.world.level.levelgen;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.concurrent.atomic.AtomicLong;
import java.util.stream.IntStream;

import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.LevelReader;
import net.minecraft.world.level.ScheduledTickAccess;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;

public final class ShapeThreeGolden {

    /// Same subject position `ShapeGolden` and `ShapeReadsGolden` use, so the
    /// three harnesses differ in exactly one thing: how many positions the
    /// level will answer before it throws.
    private static final BlockPos POS = new BlockPos(0, 64, 0);

    private static final Direction[] DIRECTIONS = Direction.values();

    /// The most distinct third-position behaviours a pair may have before it
    /// is refused rather than resolved. Each one costs an exhaustive 32,366
    /// neighbour sweep, so this is a wall-clock bound and not a modelling
    /// choice. Printed in the output as `extraClassCap`.
    private static final int EXTRA_CLASS_CAP = 128;

    /// `-Drustmc.shape3.only=WallBlock` restricts the run to one block class.
    /// For measuring the cost of a family before paying for all of them; the
    /// output names the filter so a partial run cannot be read as a full one.
    private static final String ONLY = System.getProperty("rustmc.shape3.only", "");

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        SurfaceChunkGolden.bindBlockTags();
        SurfaceChunkGolden.bindFluidTags();

        List<BlockState> all = new ArrayList<>();
        for (BlockState state : Block.BLOCK_STATE_REGISTRY) {
            all.add(state);
        }
        int n = all.size();
        BlockState[] byId = new BlockState[n];
        for (BlockState state : all) {
            byId[Block.getId(state)] = state;
        }
        for (int i = 0; i < n; i++) {
            if (byId[i] == null) {
                throw new IllegalStateException("state id " + i + " is not in the registry");
            }
        }

        BlockState[] defaults = BuiltInRegistries.BLOCK.stream()
            .map(Block::defaultBlockState)
            .toArray(BlockState[]::new);
        int[] defaultIds = new int[defaults.length];
        for (int i = 0; i < defaults.length; i++) {
            defaultIds[i] = Block.getId(defaults[i]);
        }

        AtomicLong probes = new AtomicLong();
        Pair[][] pairs = new Pair[n][];
        IntStream.range(0, n).parallel()
            .forEach(id -> pairs[id] = examine(byId[id], byId, defaults, probes));

        emit(n, defaults.length, pairs, probes.get());
    }

    /// How a refused pair came out once the harness answered a third position.
    private enum Verdict {
        /// Every extra class produced at most one alternative. Rows emitted.
        CLOSED,
        /// Some extra class produced several. The connection family again,
        /// hidden behind a refusal — `update_shape.json` counts the same thing
        /// as `connective` for the pairs it could reach.
        CONNECTIVE,
        /// No extra class produced anything but the subject, over the
        /// exhaustive neighbour sweep. The pair was never a cascade.
        INERT,
        /// The rule read a fourth position, or a level method, or has more
        /// distinct third-position behaviours than the cap allows.
        REFUSED,
    }

    private static final class Row {
        private final String state;
        private final int dir;
        private final String at;
        private final String extraSet;
        private final String to;
        private final boolean schedules;
        private final String neighbours;

        private Row(String state, int dir, String at, String extraSet,
                    String to, boolean schedules, String neighbours) {
            this.state = state;
            this.dir = dir;
            this.at = at;
            this.extraSet = extraSet;
            this.to = to;
            this.schedules = schedules;
            this.neighbours = neighbours;
        }
    }

    private static final class Pair {
        private final String state;
        private final String blockClass;
        private final int dir;
        /// The offset `ShapeGolden`'s narrow probe threw on, as `x,y,z`.
        private String at;
        private Verdict verdict = Verdict.REFUSED;
        /// Why, when [`verdict`] is REFUSED.
        private String cause;
        private int extraClasses;
        private final List<Row> rows = new ArrayList<>();

        private Pair(String state, String blockClass, int dir) {
            this.state = state;
            this.blockClass = blockClass;
            this.dir = dir;
        }
    }

    /// One extra class: the third-position states that behave alike, and what
    /// the exhaustive neighbour sweep said about them.
    private static final class ExtraClass {
        private final int[] vector;
        private final byte[] members;
        private BlockState to;
        private boolean several;
        private boolean schedules;
        private byte[] neighbours;

        private ExtraClass(int[] vector, int defaults) {
            this.vector = vector;
            this.members = new byte[(defaults + 7) / 8];
        }
    }

    /// Narrow probe, then the three-position probe, for one subject state.
    private static Pair[] examine(
        BlockState subject, BlockState[] byId, BlockState[] defaults, AtomicLong probes
    ) {
        String cls = subject.getBlock().getClass().getSimpleName();
        if (!ONLY.isEmpty() && !ONLY.equals(cls)) {
            return new Pair[0];
        }
        String name = BlockStateParser.serialize(subject);
        List<Pair> found = new ArrayList<>();
        long local = 0;

        for (int d = 0; d < DIRECTIONS.length; d++) {
            // `ShapeGolden`'s discovery pass, byte for byte, so the refusal
            // set this widens is that table's and not a second opinion. That
            // means **all 32,366 states** and not the 1,196 defaults: the
            // breadth changed on 2026-08-18 and reading the old one here
            // would widen a refusal set of 79,111 while the table carries
            // 79,113, which `ci/shape-three.sh`'s assertion refuses.
            Probe narrow = new Probe(subject, DIRECTIONS[d], null);
            boolean moved = false;
            boolean refused = false;
            for (BlockState candidate : byId) {
                local++;
                try {
                    if (narrow.answer(candidate) != subject) {
                        moved = true;
                        break;
                    }
                } catch (RuntimeException e) {
                    refused = true;
                    break;
                }
            }
            if (!refused) {
                if (!moved) {
                    continue;
                }
                // Discovery found a move, so `ShapeGolden` goes on to its
                // exhaustive sweep and may refuse partway through. Reproduced
                // here **including both of its early exits**, because the two
                // interact: a sweep that meets a second alternative at id 100
                // and a refusal at id 500 is `connective` in that table and
                // must not be counted as refused here. Getting this wrong is
                // silent — the totals still add up, against a different set.
                BlockState other = null;
                boolean several = false;
                boolean late = false;
                for (BlockState candidate : byId) {
                    local++;
                    BlockState answer;
                    try {
                        answer = narrow.answer(candidate);
                    } catch (RuntimeException e) {
                        late = true;
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
                }
                if (!late) {
                    continue;
                }
                if (several) {
                    throw new IllegalStateException("unreachable: several and late both set");
                }
            }
            Pair pair = new Pair(name, cls, d);
            if (narrow.readAt == null) {
                // A level *method*, not a position. No number of positions
                // reaches it; the census counts these and so does this.
                pair.cause = narrow.unmodelled;
                found.add(pair);
                continue;
            }
            pair.at = narrow.readAt;
            local += resolve(pair, subject, byId, defaults, DIRECTIONS[d], narrow.readOffset);
            found.add(pair);
        }
        probes.addAndGet(local);
        return found.toArray(new Pair[0]);
    }

    /// The three-position pass for one refused pair.
    private static long resolve(
        Pair pair, BlockState subject, BlockState[] byId, BlockState[] defaults,
        Direction direction, BlockPos extraPos
    ) {
        long local = 0;
        List<ExtraClass> classes = new ArrayList<>();
        int[] scratch = new int[defaults.length];

        // ---- partition the third axis over the 1,196 default neighbours ----
        for (int e = 0; e < defaults.length; e++) {
            Probe wide = new Probe(subject, direction, extraPos);
            wide.extra = defaults[e];
            for (int c = 0; c < defaults.length; c++) {
                local++;
                try {
                    scratch[c] = Block.getId(wide.answer(defaults[c]));
                } catch (RuntimeException ex) {
                    pair.cause = wide.refusal();
                    return local;
                }
            }
            ExtraClass klass = classOf(classes, scratch, defaults.length);
            if (klass == null) {
                if (classes.size() >= EXTRA_CLASS_CAP) {
                    pair.cause = "more than " + EXTRA_CLASS_CAP
                        + " distinct third-position behaviours";
                    pair.extraClasses = classes.size() + 1;
                    return local;
                }
                klass = new ExtraClass(scratch.clone(), defaults.length);
                classes.add(klass);
            }
            klass.members[e >> 3] |= (byte) (1 << (e & 7));
        }
        pair.extraClasses = classes.size();

        // ---- one exhaustive neighbour sweep per class ----------------------
        for (ExtraClass klass : classes) {
            int rep = firstMember(klass.members);
            Probe wide = new Probe(subject, direction, extraPos);
            wide.extra = defaults[rep];
            byte[] bits = new byte[(byId.length + 7) / 8];
            BlockState other = null;
            for (int id = 0; id < byId.length; id++) {
                local++;
                BlockState answer;
                try {
                    answer = wide.answer(byId[id]);
                } catch (RuntimeException ex) {
                    pair.cause = wide.refusal();
                    return local;
                }
                if (answer == subject) {
                    continue;
                }
                if (other == null) {
                    other = answer;
                } else if (answer != other) {
                    klass.several = true;
                    break;
                }
                bits[id >> 3] |= (byte) (1 << (id & 7));
            }
            klass.to = other;
            klass.schedules = wide.scheduled;
            klass.neighbours = bits;
        }

        boolean anySeveral = false;
        boolean anyMove = false;
        for (ExtraClass klass : classes) {
            anySeveral |= klass.several;
            anyMove |= klass.to != null && !klass.several;
        }
        if (anySeveral) {
            pair.verdict = Verdict.CONNECTIVE;
            return local;
        }
        if (!anyMove) {
            pair.verdict = Verdict.INERT;
            return local;
        }
        pair.verdict = Verdict.CLOSED;
        Base64.Encoder b64 = Base64.getEncoder();
        for (ExtraClass klass : classes) {
            if (klass.to == null) {
                // This third-position class is inert. Emitting no row for it
                // is the same statement `update_shape.json` makes by having no
                // row for an inert pair, and the extra-set bitmaps of the
                // classes that *are* emitted say which blocks it covered.
                continue;
            }
            pair.rows.add(new Row(
                pair.state, pair.dir, pair.at,
                b64.encodeToString(klass.members),
                BlockStateParser.serialize(klass.to),
                klass.schedules,
                b64.encodeToString(klass.neighbours)));
        }
        return local;
    }

    /// The class whose answer vector equals this one, or `null`.
    ///
    /// Compared by content. A 64-bit hash over 1,196 ints would collide about
    /// once in never, and "about once in never" is the kind of claim this
    /// project makes a check for rather than relies on: the vectors are kept
    /// and `Arrays.equals` decides.
    private static ExtraClass classOf(List<ExtraClass> classes, int[] vector, int len) {
        for (ExtraClass klass : classes) {
            if (Arrays.equals(klass.vector, 0, len, vector, 0, len)) {
                return klass;
            }
        }
        return null;
    }

    private static int firstMember(byte[] bits) {
        for (int i = 0; i < bits.length * 8; i++) {
            if ((bits[i >> 3] & (1 << (i & 7))) != 0) {
                return i;
            }
        }
        throw new IllegalStateException("an extra class with no members");
    }

    // ---- output -----------------------------------------------------------

    private static void emit(int states, int extraBreadth, Pair[][] pairs, long probes) {
        int eligible = 0;
        int rows = 0;
        Map<Verdict, Integer> verdicts = new LinkedHashMap<>();
        for (Verdict v : Verdict.values()) {
            verdicts.put(v, 0);
        }
        Map<String, Map<Verdict, Integer>> byOffset = new TreeMap<>();
        Map<String, Map<Verdict, Integer>> byClass = new TreeMap<>();
        Map<String, Integer> causes = new TreeMap<>();
        Map<String, TreeSet<String>> causeBlocks = new TreeMap<>();
        Map<Integer, Integer> byExtraClassCount = new TreeMap<>();
        List<Row> table = new ArrayList<>();

        for (Pair[] perState : pairs) {
            if (perState == null) {
                continue;
            }
            for (Pair p : perState) {
                eligible++;
                verdicts.merge(p.verdict, 1, Integer::sum);
                String offset = p.at == null ? "(method)" : p.at;
                byOffset.computeIfAbsent(offset, k -> new LinkedHashMap<>())
                    .merge(p.verdict, 1, Integer::sum);
                byClass.computeIfAbsent(p.blockClass, k -> new LinkedHashMap<>())
                    .merge(p.verdict, 1, Integer::sum);
                if (p.verdict == Verdict.REFUSED) {
                    causes.merge(p.cause, 1, Integer::sum);
                    TreeSet<String> blocks =
                        causeBlocks.computeIfAbsent(p.cause, k -> new TreeSet<>());
                    if (blocks.size() < 8) {
                        blocks.add(p.state.split("\\[")[0]);
                    }
                } else {
                    byExtraClassCount.merge(p.extraClasses, 1, Integer::sum);
                }
                table.addAll(p.rows);
                rows += p.rows.size();
            }
        }

        StringBuilder out = new StringBuilder("{\n");
        out.append("  \"rng\": \"update_shape3\",\n");
        out.append("  \"what\": \"updateShape for the pairs that read one position past their neighbour\",\n");
        out.append("  \"states\": ").append(states).append(",\n");
        out.append("  \"neighbourBreadth\": ").append(states)
            .append(", \"extraBreadth\": ").append(extraBreadth).append(",\n");
        out.append("  \"extraClassCap\": ").append(EXTRA_CLASS_CAP).append(",\n");
        out.append("  \"only\": ").append(quote(ONLY)).append(",\n");
        out.append("  \"probes\": ").append(probes).append(",\n");
        out.append("  \"directionOrder\": [");
        for (int i = 0; i < DIRECTIONS.length; i++) {
            out.append(i > 0 ? ", " : "").append(quote(DIRECTIONS[i].name()));
        }
        out.append("],\n");
        out.append("  \"refusedPairs\": ").append(eligible).append(",\n");
        out.append("  \"rows\": ").append(rows).append(",\n");
        out.append("  \"verdicts\": {");
        int seen = 0;
        for (Map.Entry<Verdict, Integer> e : verdicts.entrySet()) {
            out.append(seen++ > 0 ? ", " : "").append(quote(e.getKey().name().toLowerCase()))
                .append(": ").append(e.getValue());
        }
        out.append("},\n");
        out.append("  \"byOffset\": [\n");
        seen = 0;
        for (Map.Entry<String, Map<Verdict, Integer>> e : byOffset.entrySet()) {
            seen++;
            out.append("    {\"at\": ").append(quote(e.getKey()))
                .append(", \"pairs\": ").append(sum(e.getValue()))
                .append(", \"verdicts\": ").append(verdictMap(e.getValue()))
                .append("}").append(seen < byOffset.size() ? ",\n" : "\n");
        }
        out.append("  ],\n");
        out.append("  \"byClass\": [\n");
        seen = 0;
        for (Map.Entry<String, Map<Verdict, Integer>> e : byClass.entrySet()) {
            seen++;
            out.append("    {\"class\": ").append(quote(e.getKey()))
                .append(", \"pairs\": ").append(sum(e.getValue()))
                .append(", \"verdicts\": ").append(verdictMap(e.getValue()))
                .append("}").append(seen < byClass.size() ? ",\n" : "\n");
        }
        out.append("  ],\n");
        out.append("  \"byExtraClassCount\": {\n");
        seen = 0;
        for (Map.Entry<Integer, Integer> e : byExtraClassCount.entrySet()) {
            seen++;
            out.append("    ").append(quote(String.valueOf(e.getKey()))).append(": ")
                .append(e.getValue())
                .append(seen < byExtraClassCount.size() ? ",\n" : "\n");
        }
        out.append("  },\n");
        out.append("  \"stillRefused\": [\n");
        seen = 0;
        for (Map.Entry<String, Integer> e : causes.entrySet()) {
            seen++;
            out.append("    {\"cause\": ").append(quote(e.getKey()))
                .append(", \"pairs\": ").append(e.getValue())
                .append(", \"blocks\": ").append(strings(causeBlocks.get(e.getKey())))
                .append("}").append(seen < causes.size() ? ",\n" : "\n");
        }
        out.append("  ],\n");
        out.append("  \"table\": [\n");
        for (int i = 0; i < table.size(); i++) {
            Row r = table.get(i);
            out.append("    {\"state\": ").append(quote(r.state))
                .append(", \"dir\": ").append(r.dir)
                .append(", \"at\": ").append(quote(r.at))
                .append(", \"extra\": ").append(quote(r.extraSet))
                .append(", \"to\": ").append(quote(r.to))
                .append(", \"schedules\": ").append(r.schedules)
                .append(", \"neighbours\": ").append(quote(r.neighbours))
                .append("}").append(i + 1 < table.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    private static int sum(Map<Verdict, Integer> m) {
        return m.values().stream().mapToInt(Integer::intValue).sum();
    }

    private static String verdictMap(Map<Verdict, Integer> m) {
        StringBuilder b = new StringBuilder("{");
        int seen = 0;
        for (Verdict v : Verdict.values()) {
            Integer c = m.get(v);
            if (c == null) {
                continue;
            }
            b.append(seen++ > 0 ? ", " : "").append(quote(v.name().toLowerCase()))
                .append(": ").append(c);
        }
        return b.append("}").toString();
    }

    private static String strings(java.util.Set<String> s) {
        StringBuilder b = new StringBuilder("[");
        int seen = 0;
        for (String v : s) {
            b.append(seen++ > 0 ? ", " : "").append(quote(v));
        }
        return b.append("]").toString();
    }

    /// The subject, one neighbour, optionally **one** further position, and a
    /// level that throws for anything else.
    ///
    /// `extraPos == null` is `ShapeGolden`'s level exactly. With an
    /// `extraPos`, the level answers three positions and no more — the fourth
    /// throws, and the offset it wanted is recorded so the refusal names it.
    private static final class Probe {
        private final BlockState subject;
        private final Direction direction;
        private final BlockPos neighborPos;
        private final BlockPos extraPos;
        private final Object level;
        private final RandomSource random = RandomSource.create(0L);
        private BlockState neighbor;
        private BlockState extra;
        private boolean scheduled;
        /// The offset of the first position this level would not answer, as
        /// `x,y,z` relative to the subject, or `null` if none was asked for.
        private String readAt;
        private BlockPos readOffset;
        /// The level method this harness does not model, if one was called.
        private String unmodelled;

        private Probe(BlockState subject, Direction direction, BlockPos extraPos) {
            this.subject = subject;
            this.direction = direction;
            this.neighborPos = POS.relative(direction);
            this.extraPos = extraPos;
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
                    return extraPos == null ? "one-neighbour level" : "three-position level";
                }
                if (call.equals("hashCode")) {
                    return System.identityHashCode(proxy);
                }
                if (call.equals("equals")) {
                    return proxy == args[0];
                }
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
                String what = method.getDeclaringClass().getSimpleName() + "." + call;
                unmodelled = "updateShape read " + what;
                throw new UnsupportedOperationException(unmodelled);
            };
            this.level = Proxy.newProxyInstance(
                ShapeThreeGolden.class.getClassLoader(),
                new Class<?>[]{LevelReader.class, ScheduledTickAccess.class},
                handler);
        }

        /// What this pair could not answer, in the form the ledger keys on.
        private String refusal() {
            return readAt != null
                ? "updateShape read " + readAt + ", a fourth position"
                : unmodelled;
        }

        private BlockState stateAt(BlockPos pos) {
            if (pos.equals(POS)) {
                return subject;
            }
            if (pos.equals(neighborPos)) {
                return neighbor;
            }
            if (extraPos != null && pos.equals(extraPos)) {
                return extra;
            }
            readAt = relative(pos);
            readOffset = pos.immutable();
            throw new UnsupportedOperationException(
                "updateShape read " + readAt + ", not a position this level holds");
        }

        /// Same answer `ShapeGolden` gives, for the same reason: a `null`
        /// block entity is a shulker box with no shulker box in it.
        private Object blockEntity(BlockPos pos) {
            BlockState state = stateAt(pos);
            if (state.getBlock() instanceof net.minecraft.world.level.block.EntityBlock entity) {
                return entity.newBlockEntity(pos, state);
            }
            return null;
        }

        private static String relative(BlockPos pos) {
            return (pos.getX() - POS.getX()) + "," + (pos.getY() - POS.getY())
                + "," + (pos.getZ() - POS.getZ());
        }

        private Object ticks() {
            return Proxy.newProxyInstance(
                ShapeThreeGolden.class.getClassLoader(),
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

    private ShapeThreeGolden() {}
}
