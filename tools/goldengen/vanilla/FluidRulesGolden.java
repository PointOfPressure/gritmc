// FluidRulesGolden — the two questions `FlowingFluid` asks that are about a
// block's *kind* rather than about its geometry, plus the one that is about
// geometry and is a pair function.
//
// # What this is for
//
// `FlowingFluid.spread` and `getSpread` decide where a fluid may go through
// exactly three predicates, and none of them is portable from a block name:
//
//     canHoldAnyFluid(state)          -- private static, 116 bytes
//     canHoldSpecificFluid(level, pos, state, fluid)
//     canPassThroughWall(dir, level, from, fromState, to, toState)
//
// The first is a taxonomy: `instanceof LiquidBlockContainer`, then
// `blocksMotion`, then `instanceof DoorBlock`, then `BlockTags.SIGNS`, then
// seven named blocks. The second delegates to
// `LiquidBlockContainer.canPlaceLiquid`, which is per class. Both are pure
// functions of the block state, and this project's standing answer to a
// per-class rule over a finite domain is to ask vanilla rather than to
// transcribe it — the same argument `ShapeGolden` and `PlacementGolden` make.
//
// **They are called by reflection, so what is written down is vanilla's own
// answer and not this file's reading of it.** Both are `private static` on
// `FlowingFluid`; re-implementing them here would move the transcription
// rather than remove it, and would put it in a file no gate compares against
// anything.
//
// # `canPassThroughWall` is a pair function and it is small
//
// Its body reads the two states only through `getCollisionShape`, compares
// the results against `Shapes.block()` and `Shapes.empty()` **by identity**,
// and otherwise calls `Shapes.mergedFaceOccludes`. The two `SharedConstants`
// debug flags at offsets 0-20 are false in any production jar. So the answer
// is a pure function of
//
//     (direction, collisionShape(from), collisionShape(to))
//
// and the shapes collapse hard: 32,366 states are **318 distinct collision
// geometries**, so the whole table is `groups * groups * 6` bits — tens of
// kilobytes. Grouping by object identity instead gives 6,061 groups and a
// 110-million-call sweep, and buys nothing; see the note at the grouping
// itself for why geometry is not merely cheaper but correct.
//
// A port that computed this instead would have to reproduce `SliceShape`,
// `joinUnoptimized` and the `IndexMerger`'s 1e-7 fuzz. That is three more
// pieces of geometry to be silently wrong in, for an answer vanilla will
// simply hand over.
//
// # Refusals
//
// A state whose block `hasDynamicShape()` has no fixed collision shape — a
// moving piston is the whole of that family — and is recorded as group -1
// rather than given a plausible one. `canHoldSpecificFluid` is called against
// `EmptyBlockGetter`, and a `LiquidBlockContainer` whose `canPlaceLiquid`
// reads the level throws; that is recorded as a refusal too. Neither is
// guessed. See `docs/ORACLE.md` — a refusal is a finding and a plausible
// false is a defect.
//
// Converted to JSON by `ci/fluid-rules-golden.sh`.

package net.minecraft.world.level.levelgen;

import it.unimi.dsi.fastutil.doubles.DoubleList;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.Fluid;
import net.minecraft.world.level.material.FlowingFluid;
import net.minecraft.world.level.material.Fluids;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.shapes.VoxelShape;

public final class FluidRulesGolden {

    /// `Direction.values()` — the order every face bitstring in this project
    /// is written in.
    private static final Direction[] DIRECTIONS = Direction.values();

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        SurfaceChunkGolden.bindBlockTags();
        SurfaceChunkGolden.bindFluidTags();

        Method canHoldAny = FlowingFluid.class
            .getDeclaredMethod("canHoldAnyFluid", BlockState.class);
        canHoldAny.setAccessible(true);
        Method canHoldSpecific = FlowingFluid.class.getDeclaredMethod(
            "canHoldSpecificFluid", BlockGetter.class, BlockPos.class, BlockState.class,
            Fluid.class);
        canHoldSpecific.setAccessible(true);
        Method canPassThroughWall = FlowingFluid.class.getDeclaredMethod(
            "canPassThroughWall", Direction.class, BlockGetter.class, BlockPos.class,
            BlockState.class, BlockPos.class, BlockState.class);
        canPassThroughWall.setAccessible(true);

        List<BlockState> byId = new ArrayList<>();
        for (BlockState state : Block.BLOCK_STATE_REGISTRY) {
            byId.add(state);
        }
        int n = byId.size();
        BlockState[] states = new BlockState[n];
        for (BlockState state : byId) {
            states[Block.getId(state)] = state;
        }
        for (int i = 0; i < n; i++) {
            if (states[i] == null) {
                throw new IllegalStateException("state id " + i + " is not in the registry");
            }
        }

        // --- the two per-state predicates -------------------------------
        // **Four columns, not two.** `SimpleWaterloggedBlock.canPlaceLiquid`
        // is `fluid == Fluids.WATER` - `if_acmpne` against the **source**
        // instance, not `isSame` - so `Fluids.FLOWING_WATER` answers false
        // and flowing water cannot waterlog a fence. A table keyed on the
        // liquid rather than on the concrete fluid gets that backwards and
        // fills every fence a stream passes.
        Fluid[] probes = {Fluids.WATER, Fluids.FLOWING_WATER, Fluids.LAVA, Fluids.FLOWING_LAVA};
        String[] names = {"canHoldWater", "canHoldFlowingWater", "canHoldLava",
            "canHoldFlowingLava"};
        StringBuilder holdAny = new StringBuilder(n);
        StringBuilder[] hold = new StringBuilder[probes.length];
        for (int k = 0; k < probes.length; k++) {
            hold[k] = new StringBuilder(n);
        }
        Map<String, Integer> holdRefusals = new TreeMap<>();
        int holdRefused = 0;
        for (int i = 0; i < n; i++) {
            BlockState state = states[i];
            holdAny.append(((Boolean) canHoldAny.invoke(null, state)) ? '1' : '0');
            for (int k = 0; k < probes.length; k++) {
                try {
                    hold[k].append(specific(canHoldSpecific, state, probes[k]) ? '1' : '0');
                } catch (ReflectiveOperationException | RuntimeException e) {
                    holdRefused++;
                    Throwable cause = e.getCause() == null ? e : e.getCause();
                    holdRefusals.merge(
                        state.getBlock().getClass().getSimpleName() + ": "
                            + cause.getClass().getSimpleName(),
                        1, Integer::sum);
                    hold[k].append('0');
                }
            }
        }

        // --- the collision-shape groups ---------------------------------
        //
        // **Geometry, not object identity, and the reason is measured.**
        // The first version keyed an `IdentityHashMap` on the `VoxelShape`
        // object, reasoning that `canPassThroughWall`'s `if_acmpeq` tests see
        // identity and nothing else. Measured on 26.2 that gives **6,061
        // groups for 32,366 states**, and the pair sweep is `3 * 6061^2` =
        // 110,207,163 `canPassThroughWall` calls, which is forty-five minutes
        // per run and ninety for the two this script requires. Vanilla shares
        // shape objects far less than the `OCCLUSION_CACHE`'s 200-entry LRU
        // implies.
        //
        // Geometry is also the right key, and the identity tests are an
        // optimisation rather than semantics — which is checkable rather than
        // hopeful:
        //
        //   * `toShape == Shapes.block()` returns false. A full cube that is
        //     not the singleton falls through to `mergedFaceOccludes`, whose
        //     slices cover the face, which occludes, which returns false.
        //   * both `== Shapes.empty()` returns true. Two empty non-singletons
        //     fall through to `mergedFaceOccludes` with two empty slices,
        //     which do not cover, which does not occlude, which returns true.
        //
        // Same answer down both paths, so merging equal shapes cannot change
        // a row. The representative cross-check below is what holds that
        // open: it is now testing a real merge rather than a tautology.
        Map<String, Integer> groupOf = new LinkedHashMap<>();
        List<VoxelShape> groups = new ArrayList<>();
        int[] stateGroup = new int[n];
        int dynamic = 0;
        for (int i = 0; i < n; i++) {
            BlockState state = states[i];
            if (state.getBlock().hasDynamicShape()) {
                stateGroup[i] = -1;
                dynamic++;
                continue;
            }
            VoxelShape shape = state.getCollisionShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO);
            String key = signature(shape);
            Integer g = groupOf.get(key);
            if (g == null) {
                g = groups.size();
                groupOf.put(key, g);
                groups.add(shape);
            }
            stateGroup[i] = g;
        }
        int gn = groups.size();
        // **Nothing may be printed here.** A `System.err.println` sat at this
        // line for one afternoon as a progress note, and `Bootstrap.bootStrap`
        // installs a log4j handler that re-emits stderr onto **stdout** with a
        // `[15:20:51] [main/INFO]: [STDERR]:` prefix. That put a wall-clock
        // timestamp in the first line of the JSON, so the script's two-run
        // agreement check failed at byte 6 of line 1 and reported the dumper
        // as reading something ambient. It was: the clock, via the diagnostic
        // added to debug it. The counts this printed are in the JSON below.
        //
        // `ci/collision-shapes-golden.sh`'s header warns about exactly this
        // and this file inherited the warning without heeding it.

        // One representative state per group, so the reflective call gets the
        // BlockStates its signature wants. Which representative cannot matter
        // — the method reads nothing else off them — and that is asserted
        // below rather than asserted here in prose.
        BlockState[] representative = new BlockState[gn];
        for (int i = 0; i < n; i++) {
            int g = stateGroup[i];
            if (g >= 0 && representative[g] == null) {
                representative[g] = states[i];
            }
        }

        // --- canPassThroughWall over every ordered pair, per direction ---
        //
        // `from` sits at the origin and `to` one step along the direction,
        // which is the geometry every call site has: `getNewLiquid` probes
        // `pos.relative(d)` and `spread` probes `pos.below()`.
        //
        // **Only the three POSITIVE directions are computed and the other
        // three are their transposes.** `mergedFaceOccludes` picks
        // `a = axisDirection == POSITIVE ? shape1 : shape2` at offsets 27-40
        // and `b` the other way at 42-55, and every other line of
        // `canPassThroughWall` is symmetric in the two states. So
        //
        //     wall[NEGATIVE_d][a][b] == wall[POSITIVE_d][b][a]
        //
        // exactly, and computing both halves is 2x the work for a table that
        // is its own transpose. That is asserted rather than asserted-in-prose
        // — see the `transposeChecks` sweep below, which recomputes a sample
        // of the negative directions from vanilla and compares.
        BlockPos from = BlockPos.ZERO;
        Map<String, boolean[]> table = new LinkedHashMap<>();
        for (Direction d : DIRECTIONS) {
            if (d.getAxisDirection() != Direction.AxisDirection.POSITIVE) {
                continue;
            }
            BlockPos to = from.relative(d);
            boolean[] bits = new boolean[gn * gn];
            for (int a = 0; a < gn; a++) {
                for (int b = 0; b < gn; b++) {
                    bits[a * gn + b] = (Boolean) canPassThroughWall.invoke(
                        null, d, EmptyBlockGetter.INSTANCE, from, representative[a],
                        to, representative[b]);
                }
            }
            table.put(d.name(), bits);
        }
        Map<String, String> wall = new LinkedHashMap<>();
        for (Direction d : DIRECTIONS) {
            boolean[] bits;
            if (d.getAxisDirection() == Direction.AxisDirection.POSITIVE) {
                bits = table.get(d.name());
            } else {
                boolean[] src = table.get(d.getOpposite().name());
                bits = new boolean[gn * gn];
                for (int a = 0; a < gn; a++) {
                    for (int b = 0; b < gn; b++) {
                        bits[a * gn + b] = src[b * gn + a];
                    }
                }
            }
            StringBuilder sb = new StringBuilder(gn * gn);
            for (boolean bit : bits) {
                sb.append(bit ? '1' : '0');
            }
            wall.put(d.name(), hex(sb));
        }

        // The transpose is an assertion, not a belief. A fixed stride over
        // the pair space so the sample is spread rather than clustered at the
        // low group indices, which are the cheap all-empty shapes.
        int transposeChecks = 0;
        int stride = Math.max(1, gn / 97);
        for (Direction d : DIRECTIONS) {
            if (d.getAxisDirection() == Direction.AxisDirection.POSITIVE) {
                continue;
            }
            BlockPos to = from.relative(d);
            boolean[] derived = null;
            StringBuilder ignored = new StringBuilder();
            for (int a = 0; a < gn; a += stride) {
                for (int b = 0; b < gn; b += stride) {
                    boolean actual = (Boolean) canPassThroughWall.invoke(
                        null, d, EmptyBlockGetter.INSTANCE, from, representative[a],
                        to, representative[b]);
                    boolean claimed = table.get(d.getOpposite().name())[b * gn + a];
                    if (actual != claimed) {
                        throw new IllegalStateException(
                            "canPassThroughWall is not its own transpose at " + d + " "
                                + representative[a] + " / " + representative[b]);
                    }
                    transposeChecks++;
                }
            }
            if (derived != null || ignored.length() > 0) {
                throw new IllegalStateException("unreachable");
            }
        }

        // --- the control: the representative may not matter ---------------
        //
        // If two states in one group disagreed about `canPassThroughWall`,
        // the grouping would be wrong and every row of the table above would
        // be "whichever state happened to be first". Checked against a second
        // representative for every group that has one, in all six directions,
        // against a fixed probe partner.
        int crossChecked = 0;
        BlockState[] second = new BlockState[gn];
        for (int i = n - 1; i >= 0; i--) {
            int g = stateGroup[i];
            if (g >= 0 && second[g] == null && states[i] != representative[g]) {
                second[g] = states[i];
            }
        }
        int withSecond = 0;
        for (BlockState st : second) {
            if (st != null) {
                withSecond++;
            }
        }
        // Every group with a second member, every direction, against a
        // strided sample of partners. Exhaustive in the group being checked
        // — which is the thing that could be wrong — and sampled in the
        // partner, because the partner only ever enters through its own
        // shape and the sweep is gn^2 in full.
        int probeStride = Math.max(1, gn / 37);
        for (int g = 0; g < gn; g++) {
            if (second[g] == null) {
                continue;
            }
            for (Direction d : DIRECTIONS) {
                BlockPos to = from.relative(d);
                for (int probe = 0; probe < gn; probe += probeStride) {
                    boolean one = (Boolean) canPassThroughWall.invoke(
                        null, d, EmptyBlockGetter.INSTANCE, from, representative[g], to,
                        representative[probe]);
                    boolean two = (Boolean) canPassThroughWall.invoke(
                        null, d, EmptyBlockGetter.INSTANCE, from, second[g], to,
                        representative[probe]);
                    if (one != two) {
                        throw new IllegalStateException(
                            "group " + g + " is not a group: " + representative[g] + " and "
                                + second[g] + " disagree about " + d);
                    }
                    crossChecked++;
                }
            }
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"what\": \"FlowingFluid.canHoldAnyFluid, canHoldSpecificFluid and ")
            .append("canPassThroughWall, called reflectively on the pinned jar\",\n");
        out.append("  \"states\": ").append(n).append(",\n");
        out.append("  \"shapeGroups\": ").append(gn).append(",\n");
        out.append("  \"dynamicStates\": ").append(dynamic).append(",\n");
        out.append("  \"holdRefused\": ").append(holdRefused).append(",\n");
        out.append("  \"holdRefusedBy\": {");
        boolean first = true;
        for (Map.Entry<String, Integer> e : holdRefusals.entrySet()) {
            if (!first) {
                out.append(",");
            }
            first = false;
            out.append("\n    \"").append(e.getKey()).append("\": ").append(e.getValue());
        }
        out.append(holdRefusals.isEmpty() ? "" : "\n  ").append("},\n");
        out.append("  \"crossChecks\": ").append(crossChecked).append(",\n");
        out.append("  \"groupsWithASecondMember\": ").append(withSecond).append(",\n");
        out.append("  \"transposeChecks\": ").append(transposeChecks).append(",\n");
        out.append("  \"directionOrder\": [");
        for (int i = 0; i < DIRECTIONS.length; i++) {
            out.append(i == 0 ? "" : ", ").append('"').append(DIRECTIONS[i].name()).append('"');
        }
        out.append("],\n");
        out.append("  \"canHoldAnyFluid\": \"").append(hex(holdAny)).append("\",\n");
        for (int k = 0; k < probes.length; k++) {
            out.append("  \"").append(names[k]).append("\": \"").append(hex(hold[k]))
                .append("\",\n");
        }
        out.append("  \"stateGroup\": [");
        for (int i = 0; i < n; i++) {
            out.append(i == 0 ? "" : ",").append(stateGroup[i]);
        }
        out.append("],\n");
        out.append("  \"canPassThroughWall\": {");
        first = true;
        for (Map.Entry<String, String> e : wall.entrySet()) {
            if (!first) {
                out.append(",");
            }
            first = false;
            out.append("\n    \"").append(e.getKey()).append("\": \"").append(e.getValue())
                .append('"');
        }
        out.append("\n  }\n}\n");
        System.out.print(out);
    }

    /// The full geometry of a shape: all three coordinate lists and the box
    /// list, by raw bits so that `-0.0` and `0.0` are different keys exactly
    /// as they are different doubles. The same key
    /// `CollisionShapesGolden.signature` uses, for the same reason.
    private static String signature(VoxelShape shape) {
        StringBuilder sb = new StringBuilder();
        for (Direction.Axis axis : Direction.Axis.values()) {
            DoubleList coords = shape.getCoords(axis);
            for (int i = 0; i < coords.size(); i++) {
                sb.append(Double.doubleToRawLongBits(coords.getDouble(i))).append(',');
            }
            sb.append('|');
        }
        for (AABB b : shape.toAabbs()) {
            sb.append(Double.doubleToRawLongBits(b.minX)).append(',')
              .append(Double.doubleToRawLongBits(b.minY)).append(',')
              .append(Double.doubleToRawLongBits(b.minZ)).append(',')
              .append(Double.doubleToRawLongBits(b.maxX)).append(',')
              .append(Double.doubleToRawLongBits(b.maxY)).append(',')
              .append(Double.doubleToRawLongBits(b.maxZ)).append(';');
        }
        return sb.toString();
    }

    private static boolean specific(Method m, BlockState state, Fluid fluid)
            throws ReflectiveOperationException {
        return (Boolean) m.invoke(null, EmptyBlockGetter.INSTANCE, BlockPos.ZERO, state, fluid);
    }

    /// `1`/`0`/`?` characters to hex nibbles, MSB first, `?` written as a set
    /// bit is never done — a refused cell is `0` here and the count in
    /// `holdRefused` is what says so. Padded with zeroes to a nibble
    /// boundary; the consumer knows the length.
    private static String hex(CharSequence bits) {
        StringBuilder sb = new StringBuilder((bits.length() + 3) / 4);
        for (int i = 0; i < bits.length(); i += 4) {
            int nibble = 0;
            for (int b = 0; b < 4; b++) {
                nibble <<= 1;
                if (i + b < bits.length() && bits.charAt(i + b) == '1') {
                    nibble |= 1;
                }
            }
            sb.append(Character.forDigit(nibble, 16));
        }
        return sb.toString();
    }
}
