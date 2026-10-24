// MobPathGolden — the A* and its containers, driven from the jar without a
// server.
//
// `PathFinder.findPath` needs a `NodeEvaluator`, a `PathNavigationRegion`
// and a `Mob`, but only the evaluator is *used* by the search itself: the
// region and the mob are handed straight to `NodeEvaluator.prepare`. So a
// `NodeEvaluator` subclass that reads a fixed grid and ignores both lets
// the real `PathFinder`, `BinaryHeap`, `Node`, `Target` and `Path` run
// exactly as they do in a server, and their answers are the vectors.
// `WalkNodeEvaluator` itself is NOT exercised here — it needs a `Mob` in a
// `Level` — and the Rust test that consumes this file says so by name.
//
// What it dumps:
//
//   grids   a set of pathfinding cases: the grid (one string per z row per
//           y layer, one char per cell), start, targets in the order the
//           Java `Set<BlockPos>` handed them over (a LinkedHashSet, so the
//           order written), the search parameters, and the path the jar
//           produced: nodes (x, y, z, type, costMalus, g, h, f, walked),
//           reached, distToTarget bits, target.
//   heap    a `BinaryHeap` replay: nodes inserted with recorded f values,
//           then recorded changeCost operations, then popped to empty; the
//           pop order is the vector. Ties are the point.
//   hash    `Node.createHash` on a spread of positions, both signs, y past
//           255.
//
// The grid evaluator's neighbour rule is the same simple rule on both
// sides and is documented in the Rust test; it is deliberately not
// WalkNodeEvaluator's, so that nothing here can be mistaken for a check
// on that class.
//
// Converted to JSON by `ci/mob-path-golden.sh`.

package rustmc.golden;

import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Random;
import java.util.Set;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.world.entity.Mob;
import net.minecraft.world.level.PathNavigationRegion;
import net.minecraft.world.level.pathfinder.BinaryHeap;
import net.minecraft.world.level.pathfinder.Node;
import net.minecraft.world.level.pathfinder.NodeEvaluator;
import net.minecraft.world.level.pathfinder.Path;
import net.minecraft.world.level.pathfinder.PathFinder;
import net.minecraft.world.level.pathfinder.PathType;
import net.minecraft.world.level.pathfinder.PathfindingContext;
import net.minecraft.world.level.pathfinder.Target;

public final class MobPathGolden {

    /// Cell characters -> PathType. Every type used here has its own
    /// malus taken from PathType.getMalus(), so the vector also pins those.
    static PathType cell(char c) {
        switch (c) {
            case '.': return PathType.WALKABLE;
            case '#': return PathType.BLOCKED;
            case 'w': return PathType.WATER;
            case 'f': return PathType.FIRE;
            case 'n': return PathType.FIRE_IN_NEIGHBOR;
            case 'o': return PathType.OPEN;
            case 'd': return PathType.DAMAGING;
            case 'b': return PathType.BREACH;
            case 'h': return PathType.STICKY_HONEY;
            default: throw new IllegalArgumentException("cell " + c);
        }
    }

    /// A grid evaluator: layers[y][z].charAt(x). Out of bounds is BLOCKED.
    static final class GridEvaluator extends NodeEvaluator {
        final String[][] layers;
        final int sx, sy, sz;

        GridEvaluator(String[][] layers, int sx, int sy, int sz) {
            this.layers = layers;
            this.sx = sx;
            this.sy = sy;
            this.sz = sz;
        }

        PathType at(int x, int y, int z) {
            if (y < 0 || y >= layers.length) return PathType.BLOCKED;
            String[] layer = layers[y];
            if (z < 0 || z >= layer.length) return PathType.BLOCKED;
            String row = layer[z];
            if (x < 0 || x >= row.length()) return PathType.BLOCKED;
            return cell(row.charAt(x));
        }

        Node typed(int x, int y, int z) {
            Node n = this.getNode(x, y, z);
            PathType t = at(x, y, z);
            n.type = t;
            n.costMalus = t.getMalus();
            return n;
        }

        @Override
        public void prepare(PathNavigationRegion region, Mob mob) {
            // The base would read mob.getBbWidth(); there is no mob.
            this.nodes.clear();
        }

        @Override
        public void done() {
        }

        @Override
        public Node getStart() {
            return typed(sx, sy, sz);
        }

        @Override
        public Target getTarget(double x, double y, double z) {
            return this.getTargetNodeAt(x, y, z);
        }

        /// Neighbours, in Direction.Plane.HORIZONTAL order (NORTH, EAST,
        /// SOUTH, WEST): the same-level cell if its malus is >= 0; else
        /// the cell one up if that one's malus is >= 0 (a jump); if the
        /// same-level cell is OPEN (air), the cell one down instead if its
        /// malus is >= 0 (a drop). A neighbour already closed is skipped.
        /// Then the four diagonals (dir + clockwise), taken only when both
        /// orthogonal neighbours were accepted at the same level, and only
        /// at the same level.
        @Override
        public int getNeighbors(Node[] out, Node node) {
            int n = 0;
            Node[] ortho = new Node[4];
            int i = 0;
            for (Direction d : Direction.Plane.HORIZONTAL) {
                int x = node.x + d.getStepX();
                int z = node.z + d.getStepZ();
                Node pick = accept(x, node.y, z);
                ortho[i++] = pick;
                if (pick != null && !pick.closed) out[n++] = pick;
            }
            i = 0;
            for (Direction d : Direction.Plane.HORIZONTAL) {
                Direction cw = d.getClockWise();
                int j = (i + 1) % 4;
                Node a = ortho[i];
                Node b = ortho[j];
                i++;
                if (a == null || b == null || a.y != node.y || b.y != node.y) continue;
                int x = node.x + d.getStepX() + cw.getStepX();
                int z = node.z + d.getStepZ() + cw.getStepZ();
                if (at(x, node.y, z).getMalus() < 0) continue;
                Node diag = typed(x, node.y, z);
                if (!diag.closed) out[n++] = diag;
            }
            return n;
        }

        Node accept(int x, int y, int z) {
            PathType t = at(x, y, z);
            if (t.getMalus() >= 0 && t != PathType.OPEN) return typed(x, y, z);
            if (t == PathType.OPEN) {
                if (at(x, y - 1, z).getMalus() >= 0 && at(x, y - 1, z) != PathType.OPEN) return typed(x, y - 1, z);
                return null;
            }
            if (at(x, y + 1, z).getMalus() >= 0 && at(x, y + 1, z) != PathType.OPEN) return typed(x, y + 1, z);
            return null;
        }

        @Override
        public PathType getPathTypeOfMob(PathfindingContext ctx, int x, int y, int z, Mob mob) {
            return at(x, y, z);
        }

        @Override
        public PathType getPathType(PathfindingContext ctx, int x, int y, int z) {
            return at(x, y, z);
        }
    }

    static String q(String s) {
        return "\"" + s + "\"";
    }

    static void emitCase(StringBuilder out, String name, String[][] layers, int sx, int sy, int sz,
                         int[][] targets, float maxRange, int accuracy, float depthMul, int maxVisited) {
        GridEvaluator eval = new GridEvaluator(layers, sx, sy, sz);
        PathFinder finder = new PathFinder(eval, maxVisited);
        Set<BlockPos> set = new LinkedHashSet<>();
        for (int[] t : targets) set.add(new BlockPos(t[0], t[1], t[2]));
        Path path = finder.findPath(null, null, set, maxRange, accuracy, depthMul);
        out.append("    {\"name\": ").append(q(name)).append(",\n");
        out.append("     \"layers\": [");
        for (int y = 0; y < layers.length; y++) {
            out.append(y == 0 ? "[" : ", [");
            for (int z = 0; z < layers[y].length; z++) {
                out.append(z == 0 ? "" : ", ").append(q(layers[y][z]));
            }
            out.append("]");
        }
        out.append("],\n");
        out.append("     \"start\": [").append(sx).append(", ").append(sy).append(", ").append(sz).append("],\n");
        out.append("     \"targets\": [");
        for (int i = 0; i < targets.length; i++) {
            out.append(i == 0 ? "" : ", ").append("[").append(targets[i][0]).append(", ")
               .append(targets[i][1]).append(", ").append(targets[i][2]).append("]");
        }
        out.append("],\n");
        out.append("     \"max_range\": ").append(Float.floatToRawIntBits(maxRange))
           .append(", \"accuracy\": ").append(accuracy)
           .append(", \"depth_mul\": ").append(Float.floatToRawIntBits(depthMul))
           .append(", \"max_visited\": ").append(maxVisited).append(",\n");
        if (path == null) {
            out.append("     \"path\": null}");
            return;
        }
        out.append("     \"path\": {\"reached\": ").append(path.canReach())
           .append(", \"dist_to_target\": ").append(Float.floatToRawIntBits(path.getDistToTarget()))
           .append(", \"target\": [").append(path.getTarget().getX()).append(", ")
           .append(path.getTarget().getY()).append(", ").append(path.getTarget().getZ()).append("],\n");
        out.append("              \"nodes\": [");
        for (int i = 0; i < path.getNodeCount(); i++) {
            Node n = path.getNode(i);
            out.append(i == 0 ? "" : ",\n                        ")
               .append("[").append(n.x).append(", ").append(n.y).append(", ").append(n.z)
               .append(", ").append(q(n.type.name()))
               .append(", ").append(Float.floatToRawIntBits(n.costMalus))
               .append(", ").append(Float.floatToRawIntBits(n.g))
               .append(", ").append(Float.floatToRawIntBits(n.h))
               .append(", ").append(Float.floatToRawIntBits(n.f))
               .append(", ").append(Float.floatToRawIntBits(n.walkedDistance))
               .append("]");
        }
        out.append("]}}");
    }

    public static void main(String[] args) {
        StringBuilder out = new StringBuilder("{\n  \"rng\": \"mob_path\",\n");

        out.append("  \"grids\": [\n");
        List<String> cases = new ArrayList<>();
        StringBuilder c = new StringBuilder();

        // 1. Open room, one target, straight line with diagonal ties.
        emitCase(c, "open-room", new String[][] {{
            "........", "........", "........", "........", "........"}},
            0, 0, 2, new int[][] {{7, 0, 2}}, 16f, 0, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        // 2. A wall with a gap, the gap off the straight line.
        emitCase(c, "wall-with-gap", new String[][] {{
            "....#...", "....#...", "....#...", "........", "....#..."}},
            0, 0, 1, new int[][] {{7, 0, 1}}, 16f, 0, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        // 3. Water on the short way, dry on the long way: malus 8 per node.
        emitCase(c, "water-detour", new String[][] {{
            "........", "..wwww..", "..wwww..", "........", "........"}},
            0, 0, 1, new int[][] {{7, 0, 1}}, 16f, 0, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        // 4. Fire beside the path, FIRE_IN_NEIGHBOR on the lane next to it.
        emitCase(c, "fire-lane", new String[][] {{
            "ffffffff", "nnnnnnnn", "........", "nnnnnnnn", "ffffffff"}},
            0, 0, 1, new int[][] {{7, 0, 1}}, 16f, 0, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        // 5. Unreachable target: the closest approach, unreached.
        emitCase(c, "walled-off", new String[][] {{
            "....#...", "....#...", "....#...", "....#...", "....#..."}},
            0, 0, 2, new int[][] {{7, 0, 2}}, 16f, 0, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        // 6. Two targets, one nearer: min over reached by node count.
        emitCase(c, "two-targets", new String[][] {{
            "........", "........", "........", "........", "........"}},
            0, 0, 2, new int[][] {{7, 0, 0}, {5, 0, 4}}, 16f, 0, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        // 7. Three targets none reachable: min by distToTarget then count.
        emitCase(c, "three-unreached", new String[][] {{
            "...#....", "...#....", "...#....", "...#....", "...#...."}},
            0, 0, 2, new int[][] {{7, 0, 0}, {7, 0, 4}, {5, 0, 2}}, 16f, 0, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        // 8. Two equidistant reachable targets: the tie goes to the hash walk.
        emitCase(c, "tied-targets", new String[][] {{
            "........", "........", "........", "........", "........"}},
            3, 0, 2, new int[][] {{3, 0, 0}, {3, 0, 4}, {1, 0, 2}, {5, 0, 2}}, 16f, 0, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        // 9. Visited-node budget too small: search stops early.
        emitCase(c, "budget-8", new String[][] {{
            "........", "........", "........", "........", "........"}},
            0, 0, 2, new int[][] {{7, 0, 2}}, 16f, 0, 1f, 8);
        cases.add(c.toString()); c.setLength(0);

        // 10. Range too short: nodes past maxRange are not expanded.
        emitCase(c, "range-3", new String[][] {{
            "........", "........", "........", "........", "........"}},
            0, 0, 2, new int[][] {{7, 0, 2}}, 3f, 0, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        // 11. Accuracy 1: stop one block short.
        emitCase(c, "accuracy-1", new String[][] {{
            "........", "........", "........", "........", "........"}},
            0, 0, 2, new int[][] {{7, 0, 2}}, 16f, 1, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        // 12. Two layers: a step up onto a platform and a drop off it.
        emitCase(c, "step-and-drop", new String[][] {
            {"........", "........", "........"},
            {"oo...ooo", "oo...ooo", "oo...ooo"}},
            0, 0, 1, new int[][] {{7, 0, 1}}, 16f, 0, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        // 13. Breach and honey maluses on the direct route.
        emitCase(c, "breach-honey", new String[][] {{
            "........", ".bbbbbb.", ".hhhhhh.", "........"}},
            0, 0, 1, new int[][] {{7, 0, 2}}, 16f, 0, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        // 14. Depth multiplier halves the budget: 20 * 0.5f = 10.
        emitCase(c, "depth-half", new String[][] {{
            "........", "........", "........", "........", "........"}},
            0, 0, 2, new int[][] {{7, 0, 2}}, 16f, 0, 0.5f, 20);
        cases.add(c.toString()); c.setLength(0);

        // 15. Fifteen targets so the key set resizes to 32 buckets.
        int[][] many = new int[15][];
        for (int i = 0; i < 15; i++) many[i] = new int[] {i % 8, 0, 4 - (i % 5)};
        emitCase(c, "fifteen-targets", new String[][] {{
            "........", "........", "........", "........", "........"}},
            3, 0, 2, many, 16f, 0, 1f, 256);
        cases.add(c.toString()); c.setLength(0);

        for (int i = 0; i < cases.size(); i++) out.append(i == 0 ? "" : ",\n").append(cases.get(i));
        out.append("\n  ],\n");

        // BinaryHeap replay.
        Random rng = new Random(20260904L);
        int count = 200;
        Node[] nodes = new Node[count];
        BinaryHeap heap = new BinaryHeap();
        out.append("  \"heap\": {\"inserts\": [");
        for (int i = 0; i < count; i++) {
            nodes[i] = new Node(i, 0, 0);
            nodes[i].f = (float) rng.nextInt(20);
            heap.insert(nodes[i]);
            out.append(i == 0 ? "" : ", ").append(Float.floatToRawIntBits(nodes[i].f));
        }
        out.append("],\n           \"changes\": [");
        int changes = 0;
        for (int i = 0; i < 60; i++) {
            int idx = rng.nextInt(count);
            float f = (float) rng.nextInt(20);
            if (!nodes[idx].inOpenSet()) continue;
            heap.changeCost(nodes[idx], f);
            out.append(changes == 0 ? "" : ", ").append("[").append(idx).append(", ")
               .append(Float.floatToRawIntBits(f)).append("]");
            changes++;
        }
        // Pop a third, then remove ten by index, then pop the rest.
        out.append("],\n           \"pops_before_remove\": ").append(count / 3);
        out.append(",\n           \"pop_order\": [");
        int popped = 0;
        for (int i = 0; i < count / 3; i++) {
            Node n = heap.pop();
            out.append(popped == 0 ? "" : ", ").append(n.x);
            popped++;
        }
        out.append("],\n           \"removes\": [");
        int removed = 0;
        for (int i = 0; i < 10; i++) {
            int idx = rng.nextInt(count);
            if (!nodes[idx].inOpenSet()) continue;
            heap.remove(nodes[idx]);
            out.append(removed == 0 ? "" : ", ").append(idx);
            removed++;
        }
        out.append("],\n           \"pop_order_after\": [");
        popped = 0;
        while (!heap.isEmpty()) {
            Node n = heap.pop();
            out.append(popped == 0 ? "" : ", ").append(n.x);
            popped++;
        }
        out.append("]},\n");

        // createHash.
        out.append("  \"hash\": [");
        int[][] positions = {
            {0, 0, 0}, {1, 2, 3}, {-1, 0, 0}, {0, 0, -1}, {-1, -1, -1}, {0, 255, 0}, {0, 256, 0},
            {0, -1, 0}, {32767, 0, 0}, {32768, 0, 0}, {-32768, 0, 0}, {0, 0, 32767}, {0, 0, 32768},
            {123456, 64, -654321}, {-30000000, 320, 30000000}, {7, -64, -7}};
        for (int i = 0; i < positions.length; i++) {
            int[] p = positions[i];
            out.append(i == 0 ? "" : ", ").append("[").append(p[0]).append(", ").append(p[1])
               .append(", ").append(p[2]).append(", ").append(Node.createHash(p[0], p[1], p[2])).append("]");
        }
        out.append("]\n}\n");
        System.out.print(out);
    }
}
