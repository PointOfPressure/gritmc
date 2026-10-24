// CollisionShapesGolden — per-state collision GEOMETRY, and the measurement
// that says how much of it `Entity.move` actually needs.
//
// `block_props.json` already carries `collisionFullFaces`: six booleans per
// state saying whether each face of the collision shape is a full square.
// Six booleans cannot express a slab, a stair, a fence post or soul sand's
// 0.875, and `Entity.move` resolves a swept AABB against the actual geometry,
// so the six booleans are not a thin version of what movement needs — they
// are a different question. This dumps the geometry.
//
// # What is dumped
//
//   shapes   the distinct collision shapes, deduplicated. Each carries
//            `coords` -- the three grid coordinate lists,
//                        `VoxelShape.getCoords(axis)`;
//            `voxels` -- the occupancy grid as a hex bitstring, bit
//                        `(x * ny + y) * nz + z` of `DiscreteVoxelShape.isFull`.
//                        Together with `coords` this IS the shape as far as
//                        collision is concerned;
//            `boxes`  -- `VoxelShape.toAabbs()`, the greedily merged box
//                        list. Carried for the callers that want geometry
//                        rather than a sweep, and NOT sufficient for the
//                        sweep — see below;
//            `large`  -- `min(axis) < 0 || max(axis) > 1` on any axis, which
//                        is `BlockStateBase.hasLargeCollisionShape` and is
//                        what lets a block in the one-block shell around a
//                        query box still collide;
//            `full`   -- shape `==` `Shapes.block()` by **identity**, which
//                        is the test `BlockCollisions.computeNext` makes and
//                        which selects a different intersection test.
//   rows     one shape index per block state, indexed by the global state id
//            — `Block.BLOCK_STATE_REGISTRY.getId`, the same number the chunk
//            sections store. Every id is present. A table that emitted only
//            the interesting states would make "absent" and "no collision"
//            the same reading, and this project has been bitten by exactly
//            that shape of hole.
//
// # How much of the shape space `Entity.move` actually needs — MEASURED
//
// Vanilla's collision is not a box list. `VoxelShape.collideX` walks a
// three-dimensional voxel grid, finds the nearest occupied column in the
// sweep direction, and returns the gap to it. A full `VoxelShape` port means
// `DiscreteVoxelShape`, `BitSetDiscreteVoxelShape`, the four `IndexMerger`s,
// `BooleanOp`, `joinUnoptimized` and `optimize` — the machinery that
// *constructs* shapes out of block-class declarations.
//
// The obvious cheap answer is a box list per state: `toAabbs()`, and collide
// each box separately, on the reasoning that the nearest occupied column is
// the nearest box face and the two eps guards in `collideX` provably never
// fire. **That reasoning is wrong, and this file is where it was caught.**
// `boxEquivalence` sweeps every distinct shape, every axis, and a probe set
// built from each shape's own grid coordinates perturbed by every epsilon the
// algorithm uses, comparing
//
//     shape.collide(axis, box, d)                      <- vanilla, the grid
//     min/max over b in toAabbs() of
//         Shapes.create(b).collide(axis, box, d)       <- vanilla, per box
//
// and on 26.2 it returns **345,705 disagreements out of 16,736,112 probes**.
// `boxEquivalence.overlapping` is the count of those where the query box
// already intersects the shape, and it is all of them, which is the
// mechanism:
//
//   `collideX` starts its scan at `findIndex(sweepMax - 1e-7) + 1` — the
//   column *after* the one the box's leading face sits in — so a box already
//   embedded in solid reports the gap to the next occupied column, not zero.
//   Which column that is depends on where the grid lines fall. A union of
//   four boxes has a grid subdivided at every box face; the same solid re-cut
//   as one merged box has no interior line, the whole thing is one column,
//   the scan starts past it and finds nothing.
//
//   Worked example, shape index 2 on 26.2 (an unlit-candle-shaped four-box
//   union): a query box at x[-1e-7, 0.2499999] sweeping +X by 0.9 is inside
//   the box spanning x[0, 1]. Vanilla stops it at 0.5625001, the gap to the
//   grid line at x=0.8125. The box list stops it nowhere and it travels the
//   full 0.9, through the far wall.
//
// So the cheap answer is refused and the table carries the **grid**: three
// coordinate lists and an occupancy bitset. That is not the same as porting
// `VoxelShape` — none of the construction machinery is needed, because the
// constructed grids are what gets extracted, and there are only a few hundred
// distinct ones across all 32,366 states. `gridEquivalence` is the same sweep
// against a reduction that reads nothing but `coords` and `voxels`, and it
// must be 0. `crates/rustmc-server/src/collide.rs` refuses to load a table
// whose `gridEquivalence.mismatches` is not 0.
//
// The probe boxes are drawn from each shape's own coordinates offset by
// {0, +/-1e-8, +/-1e-7, +/-1e-6, +/-1e-3, +/-0.3} because 1.0E-7 is the
// constant `collideX` shrinks the query box by on all four cross-axis
// lookups, and a probe grid that never lands within 1e-7 of a face would
// agree with anything. It was a +/-1e-8 probe that produced the example
// above.
//
// # The step-up coordinate list is the same fact from the other side
//
// `Entity.collectCandidateStepUpHeights` reads `VoxelShape.getCoords(Axis.Y)`
// — the shape's grid lines — and NOT its box corners. So the grid is needed
// there too, independently of the sweep. `coordsDifferFromBoxCorners` counts
// the shapes where reconstructing the coordinate list from `toAabbs()` would
// have produced the wrong candidate heights.
//
// # What this table does NOT cover, said out loud
//
//   * blocks with `hasDynamicShape()`. Vanilla gives those states no
//     `BlockStateBase.Cache` at all, precisely because their shape is not a
//     function of the state. They are listed in `dynamicBlocks` and their
//     rows carry shape index -1, which the Rust loader treats as "no answer"
//     and refuses to move an entity through rather than guessing.
//   * blocks whose `getCollisionShape` override reads the `CollisionContext`
//     or the level. `contextSensitiveBlocks` is a reflection census of every
//     block class that declares the four-argument `getCollisionShape`, so it
//     is derived from the jar rather than from a list somebody maintains.
//     A block on that list still gets a row — the row is what vanilla's own
//     cache holds — but the list is the honest statement of where a
//     state-keyed table can be wrong.
//
// The shapes are read through `BlockState.getCollisionShape(BlockGetter,
// BlockPos)`, the two-argument form, which returns `BlockStateBase.Cache`'s
// `collisionShape` when there is a cache. That cache is itself built with
// `EmptyBlockGetter.INSTANCE`, `BlockPos.ZERO` and `CollisionContext.empty()`
// — so passing those here is not a stand-in for a real world, it is the same
// call vanilla makes at bootstrap.
//
// Converted to JSON by `ci/collision-shapes-golden.sh`.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.Set;
import java.util.TreeSet;

import it.unimi.dsi.fastutil.doubles.DoubleList;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.shapes.BooleanOp;
import net.minecraft.world.phys.shapes.CollisionContext;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;

public final class CollisionShapesGolden {

    /// The epsilon `VoxelShape.collideX` and `BlockCollisions` both use.
    private static final double EPS = 1.0E-7;

    /// Offsets applied to every grid coordinate to build probe boxes. The
    /// three smallest straddle `EPS` deliberately: the whole risk of a box
    /// representation lives in the four `findIndex` calls that shrink the
    /// query box by exactly `EPS`, and a probe that never lands inside that
    /// margin cannot tell a right answer from a lucky one.
    private static final double[] NUDGES = {
        0.0, 1.0E-8, -1.0E-8, EPS, -EPS, 1.0E-6, -1.0E-6, 1.0E-3, -1.0E-3, 0.3, -0.3,
    };

    /// Sweep distances. Signed, both directions, spanning "below the
    /// 1.0E-7 dead zone" to "further than a block".
    private static final double[] DELTAS = {
        1.0E-8, -1.0E-8, EPS, -EPS, 1.0E-6, -1.0E-6, 1.0E-3, -1.0E-3,
        0.05, -0.05, 0.3, -0.3, 0.9, -0.9, 1.5, -1.5,
    };

    /// Query box extents, chosen to bracket the entities that will use this:
    /// an item (0.25), a player (0.6 x 1.8), a falling block (0.98) and one
    /// wider than a block so the cross-axis ranges span several voxels.
    private static final double[][] EXTENTS = {
        {0.25, 0.25, 0.25},
        {0.6, 1.8, 0.6},
        {0.98, 0.98, 0.98},
        {1.4, 0.4, 1.4},
    };

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // Deduplicated shape table. The key is the full geometry — all three
        // coordinate lists AND the box list — so two shapes that agree on
        // their boxes but differ on their grid lines stay distinct. Collapsing
        // them would hide exactly the step-up difference this file measures.
        Map<String, Integer> index = new LinkedHashMap<>();
        List<VoxelShape> shapes = new ArrayList<>();
        List<Grid> grids = new ArrayList<>();
        List<String> shapeRows = new ArrayList<>();

        int[] rows = new int[Block.BLOCK_STATE_REGISTRY.size()];
        List<Integer> movingPiston = new ArrayList<>();
        int states = 0;
        int dynamicStates = 0;
        int emptyStates = 0;
        int degenerateBoxes = 0;
        Set<String> dynamicBlocks = new TreeSet<>();

        for (BlockState state : Block.BLOCK_STATE_REGISTRY) {
            int id = Block.BLOCK_STATE_REGISTRY.getId(state);
            states++;

            if (state.is(net.minecraft.world.level.block.Blocks.MOVING_PISTON)) {
                // `BlockCollisions.computeNext` consults a `TYPE_EDGE` cell
                // for this block and no other, by identity. The ids are
                // dumped because they cannot be recovered from the shape
                // rows: `moving_piston` is one of 23 dynamic-shape blocks and
                // all 23 carry the same `-1`.
                movingPiston.add(id);
            }
            if (state.getBlock().hasDynamicShape()) {
                // No cache exists for these and vanilla does not pretend one
                // does. -1 is "this table has no answer", not "no collision".
                dynamicBlocks.add(BuiltInRegistries.BLOCK.getKey(state.getBlock()).toString());
                dynamicStates++;
                rows[id] = -1;
                continue;
            }

            VoxelShape shape = state.getCollisionShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO);
            if (shape.isEmpty()) {
                emptyStates++;
            }
            String key = signature(shape);
            Integer at = index.get(key);
            if (at == null) {
                at = shapes.size();
                index.put(key, at);
                shapes.add(shape);
                Grid grid = Grid.of(shape);
                grids.add(grid);
                shapeRows.add(shapeRow(shape, grid));
            }
            rows[id] = at;

            for (AABB b : shape.toAabbs()) {
                if (b.maxX - b.minX < EPS || b.maxY - b.minY < EPS || b.maxZ - b.minZ < EPS) {
                    degenerateBoxes++;
                }
            }
        }

        // The reflection census: which block classes declare the
        // four-argument `getCollisionShape`. Derived from the jar, so it
        // cannot go stale the way a hand-kept list does.
        Set<String> overrides = new TreeSet<>();
        for (Block block : BuiltInRegistries.BLOCK) {
            Class<?> declaring = declaringClassOfCollisionShape(block.getClass());
            if (declaring != null
                && !declaring.getName().endsWith("BlockBehaviour")) {
                overrides.add(declaring.getSimpleName());
            }
        }

        // --- the representation proof -------------------------------------
        long probes = 0;
        long boxMismatches = 0;
        long boxMismatchesOverlapping = 0;
        long gridMismatches = 0;
        long overlapProbes = 0;
        long overlapMismatches = 0;
        List<String> boxExamples = new ArrayList<>();
        List<String> gridExamples = new ArrayList<>();
        List<String> overlapExamples = new ArrayList<>();
        int coordsDifferFromBoxCorners = 0;

        Random rng = new Random(0x0DDC0111DEL);
        for (int s = 0; s < shapes.size(); s++) {
            VoxelShape shape = shapes.get(s);
            if (shape.isEmpty()) {
                continue;
            }
            List<AABB> boxes = shape.toAabbs();
            Grid grid = grids.get(s);
            if (!cornersMatchCoords(shape, boxes)) {
                coordsDifferFromBoxCorners++;
            }
            for (Direction.Axis axis : Direction.Axis.values()) {
                for (double[] extent : EXTENTS) {
                    for (double[] origin : probeOrigins(shape, extent, rng)) {
                        AABB box = new AABB(
                            origin[0], origin[1], origin[2],
                            origin[0] + extent[0],
                            origin[1] + extent[1],
                            origin[2] + extent[2]);
                        for (double d : DELTAS) {
                            probes++;
                            double want = shape.collide(axis, box, d);
                            long wantBits = Double.doubleToRawLongBits(want);
                            // Bit-exact on both. A tolerance here would be a
                            // tolerance on the thing being proved.
                            double viaBoxes = foldBoxes(boxes, axis, box, d);
                            if (Double.doubleToRawLongBits(viaBoxes) != wantBits) {
                                boxMismatches++;
                                if (overlaps(boxes, box)) {
                                    boxMismatchesOverlapping++;
                                }
                                if (boxExamples.size() < 4) {
                                    boxExamples.add(probeJson(
                                        s, axis, box, d, want, viaBoxes));
                                }
                            }
                            double viaGrid = grid.collide(axis, box, d);
                            if (Double.doubleToRawLongBits(viaGrid) != wantBits) {
                                gridMismatches++;
                                if (gridExamples.size() < 4) {
                                    gridExamples.add(probeJson(
                                        s, axis, box, d, want, viaGrid));
                                }
                            }
                        }
                        // `BlockCollisions.computeNext`'s admission test, on
                        // the same probe boxes. Which blocks reach the shape
                        // list at all is a separate question from what the
                        // sweep then does with them, and it is decided by
                        // `Shapes.joinIsNotEmpty(entityShape, moved, AND)` --
                        // a coordinate-merging routine with its own 1.0E-7
                        // tolerance, not the plain box overlap it looks like.
                        // Measured rather than assumed for exactly that
                        // reason.
                        overlapProbes++;
                        boolean wantIn = Shapes.joinIsNotEmpty(
                            Shapes.create(box), shape, BooleanOp.AND);
                        boolean gotIn = grid.overlaps(box);
                        if (wantIn != gotIn) {
                            overlapMismatches++;
                            if (overlapExamples.size() < 4) {
                                overlapExamples.add("{\"shape\": " + s
                                    + ", \"box\": " + boxJson(box)
                                    + ", \"want\": " + wantIn
                                    + ", \"got\": " + gotIn + "}");
                            }
                        }
                    }
                }
            }
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"collision_shapes\",\n");
        out.append("  \"states\": ").append(states).append(",\n");
        out.append("  \"distinctShapes\": ").append(shapes.size()).append(",\n");
        out.append("  \"emptyStates\": ").append(emptyStates).append(",\n");
        out.append("  \"dynamicStates\": ").append(dynamicStates).append(",\n");
        out.append("  \"degenerateBoxes\": ").append(degenerateBoxes).append(",\n");
        out.append("  \"coordsDifferFromBoxCorners\": ")
           .append(coordsDifferFromBoxCorners).append(",\n");
        out.append("  \"movingPistonStates\": [");
        for (int i = 0; i < movingPiston.size(); i++) {
            out.append(i == 0 ? "" : ", ").append(movingPiston.get(i));
        }
        out.append("],\n");
        out.append("  \"dynamicBlocks\": [");
        appendStrings(out, dynamicBlocks);
        out.append("],\n");
        out.append("  \"contextSensitiveBlocks\": [");
        appendStrings(out, overrides);
        out.append("],\n");
        out.append("  \"boxEquivalence\": {\"probes\": ").append(probes)
           .append(", \"mismatches\": ").append(boxMismatches)
           .append(", \"overlapping\": ").append(boxMismatchesOverlapping)
           .append(", \"examples\": [");
        for (int i = 0; i < boxExamples.size(); i++) {
            out.append(i == 0 ? "" : ", ").append(boxExamples.get(i));
        }
        out.append("]},\n");
        out.append("  \"gridEquivalence\": {\"probes\": ").append(probes)
           .append(", \"mismatches\": ").append(gridMismatches)
           .append(", \"examples\": [");
        for (int i = 0; i < gridExamples.size(); i++) {
            out.append(i == 0 ? "" : ", ").append(gridExamples.get(i));
        }
        out.append("]},\n");
        out.append("  \"overlapEquivalence\": {\"probes\": ").append(overlapProbes)
           .append(", \"mismatches\": ").append(overlapMismatches)
           .append(", \"examples\": [");
        for (int i = 0; i < overlapExamples.size(); i++) {
            out.append(i == 0 ? "" : ", ").append(overlapExamples.get(i));
        }
        out.append("]},\n");
        out.append("  \"shapes\": [\n");
        for (int i = 0; i < shapeRows.size(); i++) {
            out.append("    ").append(shapeRows.get(i))
               .append(i + 1 < shapeRows.size() ? ",\n" : "\n");
        }
        out.append("  ],\n");
        out.append("  \"rows\": [");
        for (int i = 0; i < rows.length; i++) {
            out.append(i == 0 ? "" : ",").append(rows[i]);
        }
        out.append("]\n}\n");
        System.out.print(out);
    }

    /// A shape reduced to what collision reads: three coordinate lists and an
    /// occupancy bit per cell.
    ///
    /// The occupancy is **reconstructed from `toAabbs()`**, not read out of
    /// `DiscreteVoxelShape`, because the merged box list is by construction a
    /// partition of the occupied cells into maximal rectangular runs — the
    /// greedy merge never emits a box covering an empty cell. Reconstructing
    /// rather than reflecting into a package-private field keeps this dumper
    /// off vanilla's internals, and it is not taken on trust: if the
    /// reconstruction were wrong, [`collide`] below would disagree with
    /// `VoxelShape.collide` and `gridEquivalence` would be non-zero.
    static final class Grid {
        final double[][] coords = new double[3][];
        final boolean[] full;
        final int[] size = new int[3];

        private Grid(VoxelShape shape) {
            Direction.Axis[] axes = Direction.Axis.values();
            for (int a = 0; a < 3; a++) {
                DoubleList c = shape.getCoords(axes[a]);
                coords[a] = new double[c.size()];
                for (int i = 0; i < c.size(); i++) {
                    coords[a][i] = c.getDouble(i);
                }
                size[a] = Math.max(0, coords[a].length - 1);
            }
            full = new boolean[Math.max(1, size[0] * size[1] * size[2])];
            for (AABB b : shape.toAabbs()) {
                int x0 = exactIndex(coords[0], b.minX);
                int x1 = exactIndex(coords[0], b.maxX);
                int y0 = exactIndex(coords[1], b.minY);
                int y1 = exactIndex(coords[1], b.maxY);
                int z0 = exactIndex(coords[2], b.minZ);
                int z1 = exactIndex(coords[2], b.maxZ);
                for (int x = x0; x < x1; x++) {
                    for (int y = y0; y < y1; y++) {
                        for (int z = z0; z < z1; z++) {
                            full[(x * size[1] + y) * size[2] + z] = true;
                        }
                    }
                }
            }
        }

        static Grid of(VoxelShape shape) {
            return new Grid(shape);
        }

        /// A merged box corner must be exactly on a grid line. Anything else
        /// means `toAabbs()` is not the partition this class assumes, and
        /// guessing the nearest line would bury that.
        private static int exactIndex(double[] coords, double v) {
            for (int i = 0; i < coords.length; i++) {
                if (Double.doubleToRawLongBits(coords[i]) == Double.doubleToRawLongBits(v)) {
                    return i;
                }
            }
            throw new IllegalStateException(
                "box corner " + v + " is not a grid line — toAabbs is not a cell partition");
        }

        boolean isFull(int x, int y, int z) {
            if (x < 0 || y < 0 || z < 0 || x >= size[0] || y >= size[1] || z >= size[2]) {
                return false;
            }
            return full[(x * size[1] + y) * size[2] + z];
        }

        /// `VoxelShape.collideX`, transcribed against this grid.
        ///
        /// `collideX` reaches its three axes through `AxisCycle`; resolved,
        /// the triple is a forward rotation — sweeping X indexes (X, Y, Z),
        /// sweeping Y indexes (Y, Z, X), sweeping Z indexes (Z, X, Y). That
        /// is asserted by the sweep rather than by reading `AxisCycle`: a
        /// wrong rotation cannot agree with `VoxelShape.collide` on a shape
        /// that is not symmetric, and 318 shapes are not all symmetric.
        double collide(Direction.Axis axis, AABB box, double d) {
            int a = axis.ordinal();
            int b = (a + 1) % 3;
            int c = (a + 2) % 3;
            if (isEmptyGrid() || Math.abs(d) < EPS) {
                return isEmptyGrid() ? d : 0.0;
            }
            double hi = max(box, a);
            double lo = min(box, a);
            int i0 = findIndex(coords[a], lo + EPS);
            int i1 = findIndex(coords[a], hi - EPS);
            int j0 = Math.max(0, findIndex(coords[b], min(box, b) + EPS));
            int j1 = Math.min(size[b], findIndex(coords[b], max(box, b) - EPS) + 1);
            int k0 = Math.max(0, findIndex(coords[c], min(box, c) + EPS));
            int k1 = Math.min(size[c], findIndex(coords[c], max(box, c) - EPS) + 1);
            if (d > 0.0) {
                for (int i = i1 + 1; i < size[a]; i++) {
                    for (int j = j0; j < j1; j++) {
                        for (int k = k0; k < k1; k++) {
                            if (at(a, i, j, k)) {
                                double t = coords[a][i] - hi;
                                return t >= -EPS ? Math.min(d, t) : d;
                            }
                        }
                    }
                }
            } else if (d < 0.0) {
                for (int i = i0 - 1; i >= 0; i--) {
                    for (int j = j0; j < j1; j++) {
                        for (int k = k0; k < k1; k++) {
                            if (at(a, i, j, k)) {
                                double t = coords[a][i + 1] - lo;
                                return t <= EPS ? Math.max(d, t) : d;
                            }
                        }
                    }
                }
            }
            return d;
        }

        private boolean at(int a, int i, int j, int k) {
            return switch (a) {
                case 0 -> isFull(i, j, k);
                case 1 -> isFull(k, i, j);
                default -> isFull(j, k, i);
            };
        }

        private boolean isEmptyGrid() {
            for (boolean f : full) {
                if (f) {
                    return false;
                }
            }
            return true;
        }

        private static double min(AABB box, int axis) {
            return box.min(Direction.Axis.values()[axis]);
        }

        private static double max(AABB box, int axis) {
            return box.max(Direction.Axis.values()[axis]);
        }

        /// `VoxelShape.findIndex`: `Mth.binarySearch(0, size + 1, i -> v <
        /// get(axis, i)) - 1`, which is the last coordinate index whose value
        /// is `<= v`, and `-1` when `v` precedes the whole list.
        private static int findIndex(double[] coords, double v) {
            int lo = 0;
            int hi = coords.length;
            while (lo < hi) {
                int mid = (lo + hi) >>> 1;
                if (v < coords[mid]) {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            return lo - 1;
        }

        /// `Shapes.joinIsNotEmpty(Shapes.create(box), this, AND)` — whether a
        /// block reaches the collider list at all.
        ///
        /// **Not plain strict overlap, and that was measured rather than
        /// assumed.** Strict overlap disagrees with vanilla on 68,551 of
        /// 1,046,007 probes, always in the same direction: it admits an
        /// overlap thinner than 1.0E-7 where vanilla does not. The mechanism
        /// is `IndirectMerger`, which builds the merged coordinate list
        /// `joinIsNotEmpty` walks and treats two coordinates within 1.0E-7 as
        /// one — so a cell thinner than that is not merely small, it does not
        /// exist. A box resting on y=1 with its bottom at 0.99999999 overlaps
        /// a full block by 1e-8 and is admitted by `<`, and by nothing else.
        ///
        /// The margin is subtracted from the far side on each axis, so the
        /// overlap must be wider than 1.0E-7 to count. Touching exactly is
        /// not overlapping, which it also is not under plain `<`.
        boolean overlaps(AABB box) {
            for (int x = 0; x < size[0]; x++) {
                for (int y = 0; y < size[1]; y++) {
                    for (int z = 0; z < size[2]; z++) {
                        if (!isFull(x, y, z)) {
                            continue;
                        }
                        if (box.minX < coords[0][x + 1] - EPS
                            && coords[0][x] < box.maxX - EPS
                            && box.minY < coords[1][y + 1] - EPS
                            && coords[1][y] < box.maxY - EPS
                            && box.minZ < coords[2][z + 1] - EPS
                            && coords[2][z] < box.maxZ - EPS) {
                            return true;
                        }
                    }
                }
            }
            return false;
        }

        String hex() {
            StringBuilder sb = new StringBuilder();
            int nibble = 0;
            int bitsIn = 0;
            for (boolean f : full) {
                nibble = (nibble << 1) | (f ? 1 : 0);
                if (++bitsIn == 4) {
                    sb.append(Character.forDigit(nibble, 16));
                    nibble = 0;
                    bitsIn = 0;
                }
            }
            if (bitsIn > 0) {
                sb.append(Character.forDigit(nibble << (4 - bitsIn), 16));
            }
            return sb.toString();
        }
    }

    /// Whether the query box intersects any of the shape's boxes — the
    /// classification that turned the box-list disagreements from a mystery
    /// into a mechanism. `AABB.intersects` is the strict test, no epsilon,
    /// matching `BlockCollisions`.
    private static boolean overlaps(List<AABB> boxes, AABB box) {
        for (AABB b : boxes) {
            if (box.intersects(b)) {
                return true;
            }
        }
        return false;
    }

    private static String probeJson(
        int shape, Direction.Axis axis, AABB box, double d, double want, double got
    ) {
        return "{\"shape\": " + shape
            + ", \"axis\": " + quote(axis.name())
            + ", \"box\": " + boxJson(box)
            + ", \"delta\": " + bits(d)
            + ", \"want\": " + bits(want)
            + ", \"got\": " + bits(got) + "}";
    }

    /// `Shapes.collide`'s fold, but over one shape's boxes instead of the
    /// shape.
    ///
    /// **No epsilon check between the boxes**, and that is the whole
    /// subtlety. `Shapes.collide` tests `abs(d) < 1.0E-7` once per *shape*;
    /// expanding a shape into N boxes and testing per box would zero a
    /// legitimately tiny result that vanilla carries out of the shape. So
    /// every box is measured against the **original** `d` and the results are
    /// reduced, which is algebraically what a single `collideX` over the
    /// union computes.
    private static double foldBoxes(List<AABB> boxes, Direction.Axis axis, AABB box, double d) {
        double best = d;
        for (AABB b : boxes) {
            double t = Shapes.create(b).collide(axis, box, d);
            best = d > 0.0 ? Math.min(best, t) : Math.max(best, t);
        }
        return best;
    }

    /// Probe box origins for one shape: every grid coordinate on every axis,
    /// nudged by every epsilon, plus a deterministic random scatter so the
    /// sweep is not confined to the coordinates it was designed around.
    private static List<double[]> probeOrigins(VoxelShape shape, double[] extent, Random rng) {
        double[][] per = new double[3][];
        Direction.Axis[] axes = Direction.Axis.values();
        for (int a = 0; a < 3; a++) {
            DoubleList coords = shape.getCoords(axes[a]);
            Set<Double> cand = new LinkedHashSet<>();
            for (int i = 0; i < coords.size(); i++) {
                double c = coords.getDouble(i);
                for (double n : NUDGES) {
                    // Both "box starts here" and "box ends here": a face
                    // flush with the far side of the query box is a
                    // different index calculation from one flush with the
                    // near side, and only one of the two is exercised if the
                    // origin alone is swept.
                    cand.add(c + n);
                    cand.add(c + n - extent[a]);
                }
            }
            for (int i = 0; i < 6; i++) {
                cand.add(-1.5 + rng.nextDouble() * 4.0);
            }
            double[] arr = new double[cand.size()];
            int k = 0;
            for (double v : cand) {
                arr[k++] = v;
            }
            per[a] = arr;
        }
        // A full cross product of three ~100-element lists is 10^6 boxes per
        // shape per extent, which is minutes per shape. Pair each axis's
        // candidates against a rotating sample of the other two instead: every
        // candidate on every axis is still visited, at N rather than N^3.
        List<double[]> out = new ArrayList<>();
        int n = Math.max(per[0].length, Math.max(per[1].length, per[2].length));
        for (int i = 0; i < n; i++) {
            for (int j = 0; j < 3; j++) {
                out.add(new double[]{
                    per[0][(i + j * 7) % per[0].length],
                    per[1][(i + j * 13) % per[1].length],
                    per[2][(i + j * 23) % per[2].length],
                });
            }
        }
        return out;
    }

    /// Whether the shape's grid lines are exactly the corners of its merged
    /// box list. When false, `collectCandidateStepUpHeights` cannot be fed
    /// from `boxes` and needs `coords`.
    private static boolean cornersMatchCoords(VoxelShape shape, List<AABB> boxes) {
        for (Direction.Axis axis : Direction.Axis.values()) {
            Set<Double> corners = new TreeSet<>();
            for (AABB b : boxes) {
                corners.add(b.min(axis));
                corners.add(b.max(axis));
            }
            DoubleList coords = shape.getCoords(axis);
            Set<Double> grid = new TreeSet<>();
            for (int i = 0; i < coords.size(); i++) {
                grid.add(coords.getDouble(i));
            }
            if (!corners.equals(grid)) {
                return false;
            }
        }
        return true;
    }

    /// The class that actually declares the four-argument `getCollisionShape`
    /// for a block, walking up from its concrete class. `null` when nothing
    /// in the chain declares it, which cannot happen while `BlockBehaviour`
    /// does — a null would mean this method is looking for the wrong
    /// signature, so the caller treats it as "no override" and the census
    /// would silently empty. Guarded by the assertion in the shell script
    /// that the census is non-empty.
    private static Class<?> declaringClassOfCollisionShape(Class<?> from) {
        for (Class<?> c = from; c != null; c = c.getSuperclass()) {
            for (java.lang.reflect.Method m : c.getDeclaredMethods()) {
                if (!m.getName().equals("getCollisionShape")) {
                    continue;
                }
                Class<?>[] p = m.getParameterTypes();
                if (p.length == 4
                    && p[0] == BlockState.class
                    && p[3] == CollisionContext.class) {
                    return c;
                }
            }
        }
        return null;
    }

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

    private static String shapeRow(VoxelShape shape, Grid grid) {
        StringBuilder sb = new StringBuilder("{\"full\": ");
        // Identity, not equality. `BlockCollisions.computeNext` compares with
        // `if_acmpne` against `Shapes.block()` and takes a different
        // intersection test when it matches, so an equal-but-distinct shape
        // is NOT the same case.
        sb.append(shape == Shapes.block());
        boolean large = false;
        for (Direction.Axis axis : Direction.Axis.values()) {
            if (!shape.isEmpty() && (shape.min(axis) < 0.0 || shape.max(axis) > 1.0)) {
                large = true;
            }
        }
        sb.append(", \"large\": ").append(large);
        sb.append(", \"boxes\": [");
        List<AABB> boxes = shape.toAabbs();
        for (int i = 0; i < boxes.size(); i++) {
            sb.append(i == 0 ? "" : ", ").append(boxJson(boxes.get(i)));
        }
        sb.append("], \"coords\": [");
        for (Direction.Axis axis : Direction.Axis.values()) {
            sb.append(axis == Direction.Axis.X ? "[" : ", [");
            DoubleList coords = shape.getCoords(axis);
            for (int i = 0; i < coords.size(); i++) {
                sb.append(i == 0 ? "" : ", ").append(bits(coords.getDouble(i)));
            }
            sb.append("]");
        }
        sb.append("], \"size\": [")
          .append(grid.size[0]).append(", ")
          .append(grid.size[1]).append(", ")
          .append(grid.size[2]).append("]");
        sb.append(", \"voxels\": ").append(quote(grid.hex()));
        sb.append("}");
        return sb.toString();
    }

    /// Every double in this file is emitted as its raw IEEE-754 bits.
    ///
    /// `Double.toString` round-trips, but only through a parser that is
    /// itself correctly rounded, and the value of that guarantee is invisible
    /// at the point of use — a reader cannot tell a bit-exact table from one
    /// that is a half-ulp out. Bits cannot be a half-ulp out.
    private static String bits(double v) {
        return Long.toString(Double.doubleToRawLongBits(v));
    }

    private static String boxJson(AABB b) {
        return "[" + bits(b.minX) + ", " + bits(b.minY) + ", " + bits(b.minZ)
            + ", " + bits(b.maxX) + ", " + bits(b.maxY) + ", " + bits(b.maxZ) + "]";
    }

    private static void appendStrings(StringBuilder out, Iterable<String> items) {
        boolean first = true;
        for (String s : items) {
            out.append(first ? "" : ", ").append(quote(s));
            first = false;
        }
    }

    private static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private CollisionShapesGolden() {}
}
