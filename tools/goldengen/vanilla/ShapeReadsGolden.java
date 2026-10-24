// ShapeReadsGolden — what the 79,113 refused `updateShape` pairs actually read,
// and how much of that refusal is debt rather than an artefact of the harness.
//
// # Why this exists
//
// `update_shape.json` refuses **79,113 of its 103,482 sensitive (state,
// direction) pairs** — 76.45%, the largest single number in either refusal
// ledger, and until this ran it had never been characterised. "Refused" was
// being read as "owed", and the two are not the same thing:
//
//   * `ShapeGolden`'s discovery pass breaks out of the candidate loop on the
//     **first** throw. A pair whose rule reaches past its own neighbour is
//     therefore recorded as refused *before anything establishes that the
//     answer ever moves*. A pair that is entirely inert — vanilla never
//     changes that state from that direction, whatever the world — is
//     indistinguishable in the output from a pair that destroys the block.
//   * A rule that reads **one** fixed extra position is a different kind of
//     debt from one that walks the world. The first is recoverable by a
//     three-position table at bounded cost; the second is not recoverable by
//     any table.
//
// So the count says how much the harness could not answer. It does not say
// how much the server owes, and nothing in the tree derived the second number
// from the first.
//
// # What this measures
//
// For every pair `ShapeGolden` refuses — re-derived here rather than read out
// of the JSON, so the two cannot drift — this runs the same rule against a
// **widened** level: one that answers `getBlockState` for every position
// instead of two, with a chosen filler block everywhere except the subject's
// own position and the neighbour that changed.
//
// Two things come back:
//
//   1. **The read set.** Every position the rule asked for, as an offset from
//      the subject, accumulated over the whole discovery sweep. This is a
//      measurement of the rule, not of the answer, and it is what says
//      whether one extra position would close the pair.
//   2. **The answer set, under each of several fillers.** Air, stone and
//      water: nothing, a solid, a fluid. If the answers agree across all
//      three the rule is not reading the world in any way that changes its
//      output over this breadth; if they disagree, the pair genuinely needs a
//      world and no widening of a two-position harness will ever answer it.
//
// # The denominator is stated in the output, because it is not exhaustive
//
// The **widened** neighbour sweep here is the 1,196 block defaults and not
// all 32,366 states. A pair this classifies as inert is inert against 1,196
// default neighbours under 3 fillers; it is **not** proof that no state in
// the game moves it. The JSON says so in `breadth` and every count carries
// it. A pair this classifies as *moving* is exact evidence in the other
// direction: something really does change it, and one witness is enough.
//
// The **narrow** sweep is a different matter and stopped being a sample on
// 2026-08-18: it is `ShapeGolden`'s discovery pass, and that pass now reads
// every one of the 32,366 states rather than the 1,196 defaults, because this
// harness measured what the narrow breadth was dropping. See below.
//
// `inertExhaustive` is the one place the breadth is widened. Pairs that come
// back inert at discovery breadth are re-swept over **all** 32,366 states
// under all fillers, because that bucket is the one a reader is most likely
// to over-read, and it is small enough to afford. A pair counted in
// `inertExhaustive` is inert over every neighbour state that exists, under
// the three fillers.
//
// # The other thing this measures: what discovery used to never look at
//
// `ShapeGolden`'s header stated a limit and nothing ever put a number on it:
//
//     a subject that reacts to some non-default state of a block and to no
//     block's default state is missed by pass 1.
//
// This harness put the number on it — **164**, of 90,878 pairs dropped, 160
// of them the double-chest pairing rule — and `ShapeGolden`'s discovery pass
// was widened to all 32,366 states in the same branch, so the hole is closed
// rather than merely counted.
//
// **The measurement stays, and this is the whole reason it stays.** A number
// that can only ever be zero once its cause is fixed is a check that has
// stopped being able to fail, and this repo has a name for those. So the
// narrow discovery here runs at **both** breadths on every pair: the 1,196
// defaults, which is what the table used to do, and all 32,366, which is what
// it does now. `missedAtDefaults` is what the old breadth would drop and the
// new one finds; it is 164 and it must stay 164, because it is a property of
// the jar and not of the loop bound. `discoveryMissed` is what the *current*
// breadth drops and something still moves — structurally 0, and reported so
// a reader can see that the two are different questions.
//
// `insensitive` is the denominator, and it is now the pairs nothing in the
// game moves rather than the pairs no *default* moves.
//
// # THE THIRD COUNT, AND WHY TWO OF THEM WERE NOT ENOUGH
//
// `lateRefusalAtDefaults` is 0 and `resolutionRefusalAtDefaults` is 2, and
// they look like the same question because both are "did the widening add a
// refusal". They are two different passes. Discovery stops at the first
// candidate that moves the answer; resolution then sweeps from the beginning
// and can meet a throw discovery stopped short of, so a count of what
// discovery refuses is not a bound on what resolution will.
//
// PARITY-ROADMAP §83.3 is that sentence with a scar on it: the census
// reported the discovery count as 0, three documents read it as "the refusal
// set is complete", a commit message predicted zero new refusals, and the
// regeneration said two. The correction went into the prose. What stayed in
// the OUTPUT was the discovery count sitting next to `missedAtDefaults = 164`
// with nothing beside it measuring the other pass — so the next reader draws
// the same inference from the same pair of numbers, and the section warning
// them not to is somewhere else.
//
// §83.3 got its 2 by subtracting two re-derived tables. That is not a
// measurement: it needs both runs to exist, a tree holding one cannot check
// it, and anything else that moved between the two runs is silently charged
// to the widening. This measures it in one run, so the three numbers are
// 164 dropped, 0 refusing in discovery, 2 refusing in resolution, and none of
// them stands in for another.
//
// # What it deliberately does not do
//
// It does not emit a table anything ports from. A widened level is a level
// that **invents** the world beyond the neighbour, and this project's
// standing rule is that an oracle which invents a plausible answer is worse
// than one that refuses. Every number here is a statement about the shape of
// the refusal, and a pair classified `recoverable` is a *candidate* for a
// three-position extract, not a row anybody may use.
//
// # Grep any diagnostic output unanchored
//
// `Bootstrap.bootStrap()` redirects both streams through log4j, which
// re-emits each line with a `[HH:MM:SS] [main/INFO]: [STDOUT]: ` prefix, so
// `grep -E "^SOMETHING"` matches nothing and exits 1 — indistinguishable from
// a clean run that found nothing. That prefix is also why this writes its
// JSON to stdout and is captured with `>` rather than through a pipe.

package net.minecraft.world.level.levelgen;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
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
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;

public final class ShapeReadsGolden {

    /// Same position `ShapeGolden` uses, so the two harnesses differ in
    /// exactly one thing: what the level answers for a third position.
    private static final BlockPos POS = new BlockPos(0, 64, 0);

    private static final Direction[] DIRECTIONS = Direction.values();

    /// Nothing, a solid, a fluid. Named in the output so a reader knows what
    /// "agrees across fillers" was measured against.
    private static final String[] FILLER_NAMES = {"air", "stone", "water"};

    /// How a pair came out once the harness could see past the neighbour.
    private enum Verdict {
        /// Still refused: the rule called a level *method* this harness does
        /// not model. Widening the world cannot help; only a real level can.
        REFUSED,
        /// The answer was the subject itself for every neighbour and every
        /// filler. No cascade at all over this breadth.
        INERT,
        /// The subject and exactly one other answer, identical under every
        /// filler. Recoverable exactly by a table keyed on one more position.
        RECOVERABLE,
        /// Several alternatives, identical under every filler. The same
        /// connection property `update_shape.json` already counts as
        /// `connective`, hidden behind a refusal.
        CONNECTIVE,
        /// The answers moved when the world beyond the neighbour moved. Real
        /// debt: no widening of a fixed-neighbourhood harness answers this.
        WORLD,
    }

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

        BlockState[] fillers = {
            Blocks.AIR.defaultBlockState(),
            Blocks.STONE.defaultBlockState(),
            Blocks.WATER.defaultBlockState(),
        };

        AtomicLong probes = new AtomicLong();
        Pair[][] pairs = new Pair[n][];
        IntStream.range(0, n).parallel()
            .forEach(id -> pairs[id] = examine(byId[id], byId, defaults, fillers, probes));

        // ---- aggregate ------------------------------------------------------
        //
        // Every count below is derived from the same walk that produced the
        // pairs. Nothing here is carried forward from `update_shape.json` and
        // nothing is typed in: `sensitive`, `carried`, `connective` and
        // `refusedPairs` are recomputed by this harness's own narrow probe and
        // `ci/shape-reads.sh` asserts they equal the table's.
        int sensitive = 0;
        int carried = 0;
        int connective = 0;
        int refusedPairs = 0;
        int inertExhaustive = 0;
        Map<Verdict, Integer> verdicts = new LinkedHashMap<>();
        for (Verdict v : Verdict.values()) {
            verdicts.put(v, 0);
        }
        /// refusal cause -> verdict -> pairs. The cross tabulation is the
        /// whole point: "48,995 pairs read the block above" says nothing
        /// about how many of them the server owes.
        Map<String, Map<Verdict, Integer>> byCause = new TreeMap<>();
        /// how many distinct extra positions the rule read -> pairs
        Map<Integer, Integer> byExtraCount = new TreeMap<>();
        /// the exact extra read set, as an offset list -> pairs
        Map<String, Integer> byExtraSet = new TreeMap<>();
        /// level methods that are still unmodelled after widening
        Map<String, Integer> byMethod = new TreeMap<>();
        /// (block class, verdict) -> pairs, so a reader can see the family
        Map<String, Map<Verdict, Integer>> byClass = new TreeMap<>();
        Map<String, TreeSet<String>> causeBlocks = new TreeMap<>();

        int discoveryMissed = 0;
        int discoveryLateRefusal = 0;
        int missedAtDefaults = 0;
        int lateRefusalAtDefaults = 0;
        int resolutionRefusalAtDefaults = 0;
        Map<String, Integer> missedByClass = new TreeMap<>();
        for (int id = 0; id < n; id++) {
            for (Pair p : pairs[id]) {
                if (p.insensitive) {
                    // Discovery is exhaustive, so nothing reaches here. Kept
                    // as a live branch rather than deleted: if the breadth is
                    // ever narrowed again these two counts start moving, and
                    // a deleted branch cannot do that.
                    if (p.discoveryMissed) {
                        discoveryMissed++;
                    }
                    if (p.discoveryLateRefusal) {
                        discoveryLateRefusal++;
                    }
                    continue;
                }
                if (p.missedAtDefaults) {
                    missedAtDefaults++;
                    missedByClass.merge(p.blockClass, 1, Integer::sum);
                }
                if (p.lateRefusalAtDefaults) {
                    lateRefusalAtDefaults++;
                    missedByClass.merge(p.blockClass + " (late refusal)",
                        1, Integer::sum);
                }
                if (p.resolutionRefusalAtDefaults) {
                    resolutionRefusalAtDefaults++;
                    missedByClass.merge(p.blockClass + " (resolution refusal)",
                        1, Integer::sum);
                }
                sensitive++;
                if (p.narrowCarried) {
                    carried++;
                    continue;
                }
                if (p.narrowConnective) {
                    connective++;
                    continue;
                }
                refusedPairs++;
                verdicts.merge(p.verdict, 1, Integer::sum);
                if (p.inertExhaustive) {
                    inertExhaustive++;
                }
                byCause.computeIfAbsent(p.cause, k -> new LinkedHashMap<>())
                    .merge(p.verdict, 1, Integer::sum);
                TreeSet<String> blocks =
                    causeBlocks.computeIfAbsent(p.cause, k -> new TreeSet<>());
                if (blocks.size() < 8) {
                    blocks.add(p.state.split("\\[")[0]);
                }
                byExtraCount.merge(p.extras.size(), 1, Integer::sum);
                byExtraSet.merge(setKey(p.extras), 1, Integer::sum);
                if (p.method != null) {
                    byMethod.merge(p.method, 1, Integer::sum);
                }
                byClass.computeIfAbsent(p.blockClass, k -> new LinkedHashMap<>())
                    .merge(p.verdict, 1, Integer::sum);
            }
        }

        StringBuilder out = new StringBuilder("{\n");
        out.append("  \"what\": \"what update_shape.json's refused pairs read, and what they owe\",\n");
        out.append("  \"states\": ").append(n).append(",\n");
        out.append("  \"breadth\": {\"neighbours\": ").append(defaults.length)
            .append(", \"note\": \"block defaults, the WIDENED probe's breadth; the narrow ")
            .append("probe and ShapeGolden's discovery both read all ").append(n)
            .append(" states, and inertExhaustive re-sweeps all of them too\"},\n");
        out.append("  \"fillers\": [");
        for (int i = 0; i < FILLER_NAMES.length; i++) {
            out.append(i > 0 ? ", " : "").append(quote(FILLER_NAMES[i]));
        }
        out.append("],\n");
        out.append("  \"probes\": ").append(probes.get()).append(",\n");
        out.append("  \"sensitive\": ").append(sensitive).append(",\n");
        out.append("  \"carried\": ").append(carried).append(",\n");
        out.append("  \"connective\": ").append(connective).append(",\n");
        out.append("  \"refusedPairs\": ").append(refusedPairs).append(",\n");
        out.append("  \"inertExhaustive\": ").append(inertExhaustive).append(",\n");
        out.append("  \"insensitive\": ").append(n * DIRECTIONS.length - sensitive)
            .append(",\n");
        out.append("  \"discoveryMissed\": ").append(discoveryMissed).append(",\n");
        out.append("  \"discoveryLateRefusal\": ").append(discoveryLateRefusal)
            .append(",\n");
        out.append("  \"missedAtDefaults\": ").append(missedAtDefaults).append(",\n");
        out.append("  \"lateRefusalAtDefaults\": ").append(lateRefusalAtDefaults)
            .append(",\n");
        out.append("  \"resolutionRefusalAtDefaults\": ")
            .append(resolutionRefusalAtDefaults).append(",\n");
        out.append("  \"discoveryMissedByClass\": {\n");
        int missedSeen = 0;
        for (Map.Entry<String, Integer> e : missedByClass.entrySet()) {
            missedSeen++;
            out.append("    ").append(quote(e.getKey())).append(": ").append(e.getValue())
                .append(missedSeen < missedByClass.size() ? ",\n" : "\n");
        }
        out.append("  },\n");
        out.append("  \"verdicts\": {\n");
        int seen = 0;
        for (Map.Entry<Verdict, Integer> e : verdicts.entrySet()) {
            seen++;
            out.append("    ").append(quote(e.getKey().name().toLowerCase()))
                .append(": ").append(e.getValue())
                .append(seen < verdicts.size() ? ",\n" : "\n");
        }
        out.append("  },\n");
        out.append("  \"byCause\": [\n");
        seen = 0;
        for (Map.Entry<String, Map<Verdict, Integer>> e : byCause.entrySet()) {
            seen++;
            int total = e.getValue().values().stream().mapToInt(Integer::intValue).sum();
            out.append("    {\"reads\": ").append(quote(e.getKey()))
                .append(", \"pairs\": ").append(total)
                .append(", \"verdicts\": ").append(verdictMap(e.getValue()))
                .append(", \"blocks\": ").append(strings(causeBlocks.get(e.getKey())))
                .append("}").append(seen < byCause.size() ? ",\n" : "\n");
        }
        out.append("  ],\n");
        out.append("  \"byExtraPositionCount\": {\n");
        seen = 0;
        for (Map.Entry<Integer, Integer> e : byExtraCount.entrySet()) {
            seen++;
            out.append("    ").append(quote(String.valueOf(e.getKey()))).append(": ")
                .append(e.getValue()).append(seen < byExtraCount.size() ? ",\n" : "\n");
        }
        out.append("  },\n");
        out.append("  \"byExtraPositionSet\": [\n");
        seen = 0;
        for (Map.Entry<String, Integer> e : byExtraSet.entrySet()) {
            seen++;
            out.append("    {\"reads\": ").append(quote(e.getKey()))
                .append(", \"pairs\": ").append(e.getValue()).append("}")
                .append(seen < byExtraSet.size() ? ",\n" : "\n");
        }
        out.append("  ],\n");
        out.append("  \"stillUnmodelled\": [\n");
        seen = 0;
        for (Map.Entry<String, Integer> e : byMethod.entrySet()) {
            seen++;
            out.append("    {\"method\": ").append(quote(e.getKey()))
                .append(", \"pairs\": ").append(e.getValue()).append("}")
                .append(seen < byMethod.size() ? ",\n" : "\n");
        }
        out.append("  ],\n");
        out.append("  \"byClass\": [\n");
        seen = 0;
        for (Map.Entry<String, Map<Verdict, Integer>> e : byClass.entrySet()) {
            seen++;
            int total = e.getValue().values().stream().mapToInt(Integer::intValue).sum();
            out.append("    {\"class\": ").append(quote(e.getKey()))
                .append(", \"pairs\": ").append(total)
                .append(", \"verdicts\": ").append(verdictMap(e.getValue()))
                .append("}").append(seen < byClass.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
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

    private static String strings(Set<String> s) {
        StringBuilder b = new StringBuilder("[");
        int seen = 0;
        for (String v : s) {
            b.append(seen++ > 0 ? ", " : "").append(quote(v));
        }
        return b.append("]").toString();
    }

    private static String setKey(Set<String> s) {
        if (s.isEmpty()) {
            return "(none)";
        }
        TreeSet<String> sorted = new TreeSet<>(s);
        return String.join(" ", sorted);
    }

    /// One (state, direction) pair, as the narrow harness saw it and as the
    /// widened one did.
    private static final class Pair {
        private final String state;
        private final String blockClass;
        private boolean narrowCarried;
        private boolean narrowConnective;
        /// `ShapeGolden`'s refusal message, verbatim, so the two ledgers key
        /// on the same string.
        private String cause;
        /// Positions read beyond the subject's own and the changed neighbour.
        private final Set<String> extras = new LinkedHashSet<>();
        /// The level method still unmodelled after widening, if any.
        private String method;
        private Verdict verdict = Verdict.REFUSED;
        private boolean inertExhaustive;
        /// Discovery dropped this pair as inert; see the header.
        private boolean insensitive;
        /// ...and something moved it after all, at the current breadth.
        /// Structurally 0 now that discovery is exhaustive, and reported
        /// anyway so that "0" is an answer rather than an absence.
        private boolean discoveryMissed;
        /// ...or reached past the harness at the current breadth.
        private boolean discoveryLateRefusal;
        /// The **1,196-default** breadth would have dropped this pair, and
        /// the exhaustive one finds a move. This is the 164, and it is a
        /// property of the jar rather than of a loop bound, so it must not
        /// go to zero when the loop bound changes.
        private boolean missedAtDefaults;
        /// ...or the exhaustive one reaches past the harness where no default
        /// did. `discoveryLateRefusal` under the old name and old breadth.
        private boolean lateRefusalAtDefaults;
        /// ...or the exhaustive one reaches past the harness in **resolution**
        /// where no default reached it in **discovery**.
        ///
        /// This is the number §83.3 arrived at by subtracting two re-derived
        /// tables — the 2 — and it is separate from `lateRefusalAtDefaults`
        /// for the reason that section is entirely about. Discovery stops at
        /// the first candidate that moves the answer; resolution then sweeps
        /// from the beginning and can meet a throw discovery stopped short of.
        /// So `lateRefusalAtDefaults` is 0 and this is 2, and a reader who
        /// finds only the first one sitting beside `missedAtDefaults = 164`
        /// concludes the widening added nothing to the refusal set — which is
        /// exactly the conclusion §83.3 exists to correct, drawn from the very
        /// pair of numbers left standing after it.
        ///
        /// A subtraction between two runs is not a measurement: it needs both
        /// runs to exist, it cannot be checked by a tree that has one, and it
        /// silently attributes to the widening anything else that moved
        /// between them. This measures it in one run instead.
        private boolean resolutionRefusalAtDefaults;

        private Pair(String state, String blockClass) {
            this.state = state;
            this.blockClass = blockClass;
        }
    }

    /// Narrow pass then widened pass, for one subject state.
    private static Pair[] examine(
        BlockState subject, BlockState[] byId, BlockState[] defaults,
        BlockState[] fillers, AtomicLong probes
    ) {
        String name = BlockStateParser.serialize(subject);
        String cls = subject.getBlock().getClass().getSimpleName();
        List<Pair> found = new ArrayList<>();
        long local = 0;

        for (int d = 0; d < DIRECTIONS.length; d++) {
            // ---- the narrow probe, byte for byte what ShapeGolden does ----
            Probe narrow = new Probe(subject, DIRECTIONS[d], null);

            // The **old** breadth first: the 1,196 block defaults, which is
            // what discovery read until 2026-08-18. Kept so the size of the
            // hole stays a measured number after the hole is closed — see the
            // header. It is cheap: it early-exits exactly as discovery does.
            boolean movedAtDefaults = false;
            boolean refusedAtDefaults = false;
            for (BlockState candidate : defaults) {
                local++;
                try {
                    if (narrow.answer(candidate) != subject) {
                        movedAtDefaults = true;
                        break;
                    }
                } catch (RuntimeException e) {
                    refusedAtDefaults = true;
                    break;
                }
            }
            boolean droppedAtDefaults = !movedAtDefaults && !refusedAtDefaults;

            // ...then the breadth discovery actually reads now.
            boolean moved = false;
            String refusal = null;
            for (BlockState candidate : byId) {
                local++;
                try {
                    if (narrow.answer(candidate) != subject) {
                        moved = true;
                        break;
                    }
                } catch (RuntimeException e) {
                    refusal = message(e);
                    break;
                }
            }
            if (refusal == null && !moved) {
                // Nothing in the game moves this pair and nothing reaches
                // past the harness. It is outside the table by measurement
                // now rather than by breadth, so there is nothing to carry.
                continue;
            }
            Pair pair = new Pair(name, cls);
            pair.missedAtDefaults = droppedAtDefaults && refusal == null;
            pair.lateRefusalAtDefaults = droppedAtDefaults && refusal != null;
            if (refusal == null) {
                // Resolution, to split carried from connective exactly as
                // `ShapeGolden` does — including its own chance of refusing
                // partway through the exhaustive sweep.
                BlockState other = null;
                boolean several = false;
                String late = null;
                for (int id = 0; id < byId.length; id++) {
                    local++;
                    BlockState answer;
                    try {
                        answer = narrow.answer(byId[id]);
                    } catch (RuntimeException e) {
                        late = message(e);
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
                if (late != null) {
                    refusal = late;
                    // Resolution refused where discovery did not. The pair is
                    // in the refusal set; whether the 1,196-default breadth
                    // would have had it at all is the separate question.
                    pair.resolutionRefusalAtDefaults = droppedAtDefaults;
                } else if (!several && other != null) {
                    pair.narrowCarried = true;
                    found.add(pair);
                    continue;
                } else {
                    pair.narrowConnective = true;
                    found.add(pair);
                    continue;
                }
            }
            pair.cause = refusal;

            // ---- the widened probe -------------------------------------
            //
            // One run per filler. The answer set is compared across fillers,
            // and the read set is accumulated across all of them.
            List<Set<String>> answerSets = new ArrayList<>();
            boolean stillRefused = false;
            for (BlockState filler : fillers) {
                Probe wide = new Probe(subject, DIRECTIONS[d], filler);
                Set<String> answers = new TreeSet<>();
                for (BlockState candidate : defaults) {
                    local++;
                    try {
                        answers.add(BlockStateParser.serialize(wide.answer(candidate)));
                    } catch (RuntimeException e) {
                        pair.method = wide.unmodelled == null ? message(e) : wide.unmodelled;
                        stillRefused = true;
                        break;
                    }
                }
                pair.extras.addAll(wide.extras);
                if (stillRefused) {
                    break;
                }
                answerSets.add(answers);
            }
            if (stillRefused) {
                pair.verdict = Verdict.REFUSED;
                found.add(pair);
                continue;
            }
            Set<String> first = answerSets.get(0);
            boolean agree = true;
            for (Set<String> s : answerSets) {
                if (!s.equals(first)) {
                    agree = false;
                    break;
                }
            }
            if (!agree) {
                pair.verdict = Verdict.WORLD;
            } else if (first.size() == 1 && first.contains(name)) {
                pair.verdict = Verdict.INERT;
            } else if (first.size() == 2 && first.contains(name)) {
                pair.verdict = Verdict.RECOVERABLE;
            } else if (first.size() == 1) {
                // Never the subject itself: every default neighbour turns it
                // into the same other state. Still one alternative, so the
                // same shape as `carried`.
                pair.verdict = Verdict.RECOVERABLE;
            } else {
                pair.verdict = Verdict.CONNECTIVE;
            }

            // The one bucket worth paying exhaustive breadth for — see the
            // header. An `inert` verdict is the one a reader will over-read.
            if (pair.verdict == Verdict.INERT) {
                boolean exhaustive = true;
                outer:
                for (BlockState filler : fillers) {
                    Probe wide = new Probe(subject, DIRECTIONS[d], filler);
                    for (BlockState candidate : byId) {
                        local++;
                        try {
                            if (wide.answer(candidate) != subject) {
                                exhaustive = false;
                                break outer;
                            }
                        } catch (RuntimeException e) {
                            exhaustive = false;
                            break outer;
                        }
                    }
                }
                if (exhaustive) {
                    pair.inertExhaustive = true;
                } else {
                    // It moved once the breadth widened, so the discovery
                    // verdict was an artefact of the breadth and is corrected
                    // here rather than published.
                    pair.verdict = Verdict.WORLD;
                }
            }
            found.add(pair);
        }
        probes.addAndGet(local);
        return found.toArray(new Pair[0]);
    }

    private static String message(RuntimeException e) {
        String m = e.getMessage();
        return m == null ? e.getClass().getName() : m;
    }

    /// The subject, one neighbour, and either nothing else (`filler == null`,
    /// which is `ShapeGolden`'s level exactly) or a world made of one block.
    private static final class Probe {
        private final BlockState subject;
        private final Direction direction;
        private final BlockPos neighborPos;
        private final BlockState filler;
        private final Object level;
        private final RandomSource random = RandomSource.create(0L);
        private BlockState neighbor;
        /// Offsets read beyond the subject and the changed neighbour.
        private final Set<String> extras = new LinkedHashSet<>();
        /// The level method that had to be refused, if one was.
        private String unmodelled;

        private Probe(BlockState subject, Direction direction, BlockState filler) {
            this.subject = subject;
            this.direction = direction;
            this.filler = filler;
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
                    return filler == null ? "one-neighbour level" : "filled level";
                }
                if (call.equals("hashCode")) {
                    return System.identityHashCode(proxy);
                }
                if (call.equals("equals")) {
                    return proxy == args[0];
                }
                if (call.equals("getBlockTicks") || call.equals("getFluidTicks")) {
                    return ticks();
                }
                if (call.equals("createTick")) {
                    return null;
                }
                if (method.isDefault()) {
                    return InvocationHandler.invokeDefault(proxy, method, args);
                }
                String what = method.getDeclaringClass().getSimpleName() + "." + call;
                unmodelled = what;
                throw new UnsupportedOperationException("updateShape read " + what);
            };
            this.level = Proxy.newProxyInstance(
                ShapeReadsGolden.class.getClassLoader(),
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
            extras.add(relative(pos));
            if (filler == null) {
                throw new UnsupportedOperationException(
                    "updateShape read " + relative(pos) + ", not its own neighbour");
            }
            return filler;
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
                ShapeReadsGolden.class.getClassLoader(),
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

    private ShapeReadsGolden() {}
}
