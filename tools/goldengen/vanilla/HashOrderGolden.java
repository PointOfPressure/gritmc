// HashOrderGolden — java.util.HashSet iteration order, from the JVM itself.
//
// `TreeFeature.place` collects its logs, foliage and roots into
// `Sets.newHashSet()`, and `TreeDecorator.Context` sorts those by y with a
// *stable* sort — so everything at one height comes out in the set's own
// order and a decorator picks from it. That order is behaviour, and the port
// reproduces it in `java_hash_set.rs` rather than borrowing a Rust set.
//
// This dumps what the real JVM does for a spread of insertion sequences: a
// straight trunk, a foliage blob, a run that forces a resize, and a run of
// pseudo-random positions. Comparing the two lists is the only way to know
// the reproduction is right rather than merely self-consistent.
//
// Converted to JSON by `ci/hash-order-golden.sh`.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;

import net.minecraft.core.BlockPos;

public final class HashOrderGolden {

    public static void main(String[] args) {
        StringBuilder out = new StringBuilder("{\n  \"rng\": \"hash_order\",\n  \"cases\": [\n");
        List<String> cases = new ArrayList<>();

        // A trunk: one column, twelve blocks. Low bits of the hash move by 31
        // per step, which is what the spread exists to mix.
        cases.add(dump("trunk", column(0, 64, 0, 12)));
        // A resize: thirteen entries takes a sixteen-bucket table to
        // thirty-two, and the split has to preserve relative order.
        cases.add(dump("resize", column(0, 64, 0, 13)));
        // Two resizes.
        cases.add(dump("resize_twice", column(0, 64, 0, 40)));
        // A foliage blob: a 5x3x5 box, which is 75 entries and three resizes.
        List<BlockPos> blob = new ArrayList<>();
        for (int y = 0; y < 3; y++) {
            for (int z = -2; z <= 2; z++) {
                for (int x = -2; x <= 2; x++) {
                    blob.add(new BlockPos(x, 70 + y, z));
                }
            }
        }
        cases.add(dump("blob", blob));
        // Negative coordinates, where the hash is negative and the unsigned
        // shift in `HashMap.hash` stops behaving like the signed one.
        cases.add(dump("negative", column(-1000, -60, -1000, 20)));
        // A scatter, so the buckets are not walked in insertion order.
        List<BlockPos> scatter = new ArrayList<>();
        long seed = 0x5DEECE66DL;
        for (int i = 0; i < 200; i++) {
            seed = (seed * 0x5DEECE66DL + 0xBL) & ((1L << 48) - 1);
            int x = (int) (seed >>> 16) % 64;
            seed = (seed * 0x5DEECE66DL + 0xBL) & ((1L << 48) - 1);
            int y = (int) (seed >>> 16) % 64;
            seed = (seed * 0x5DEECE66DL + 0xBL) & ((1L << 48) - 1);
            int z = (int) (seed >>> 16) % 64;
            scatter.add(new BlockPos(x, y, z));
        }
        cases.add(dump("scatter", scatter));

        for (int i = 0; i < cases.size(); i++) {
            out.append("    ").append(cases.get(i)).append(i + 1 < cases.size() ? ",\n" : "\n");
        }
        out.append("  ],\n  \"ops_cases\": [\n");

        List<String> opCases = treeCases();
        for (int i = 0; i < opCases.size(); i++) {
            out.append("    ").append(opCases.get(i)).append(i + 1 < opCases.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    // -----------------------------------------------------------------------
    // Treeification
    //
    // A bucket that reaches nine entries in a table of 64 or more stops being
    // a list and becomes a red-black tree, and the tree changes what comes
    // out of the iterator: `treeify` ends by moving the root to the head of
    // the bin, `putTreeVal` splices each later entry in after its tree
    // *parent* rather than at the tail, `split` carries all of that across a
    // resize, and `removeTreeNode` rebalances and moves the root again.
    //
    // None of that happens in the six cases above — they are all short
    // buckets — so a port could reproduce every one of them and still be
    // wrong about every spruce in the world. These cases exist to reach it.
    //
    // The format is an **operation stream** rather than an insertion list,
    // because removal is half of what a tree does and `TreeFeature`'s
    // leaf-distance flood removes constantly. Each case also carries a
    // fingerprint of the iteration order *after every single operation*, so a
    // port that diverges is told which operation diverged rather than being
    // told that six hundred operations later the answer was different.
    // -----------------------------------------------------------------------

    /// `HashMap.hash(key)` — the spread, so a case can aim at a bucket.
    private static int spread(BlockPos p) {
        int h = p.hashCode();
        return h ^ (h >>> 16);
    }

    /// Positions to build a case out of, drawn so that **no two share a
    /// spread hash**.
    ///
    /// That restriction is not fussiness. `treeify` orders a bin by hash and
    /// only falls through to `tieBreakOrder` — and so to
    /// `System.identityHashCode` — when two hashes are *equal*, at which
    /// point the JVM's own answer stops being reproducible from one run to
    /// the next. The first draft of these cases picked positions from a
    /// ±128 box, where `961z + 31y + x` is nowhere near injective, and the
    /// two-run check in `ci/hash-order-golden.sh` caught it immediately: the
    /// vectors would have been noise.
    ///
    /// The box below is the one a tree occupies — `|x|, |z| <= 15`,
    /// `0 <= y <= 30` — and over it the hash *is* injective, because
    /// `31y + x` spans 960 values and `z` steps by 961. That is also why the
    /// village window sees no ties: a tree's positions are local.
    private static final class Space {
        private final java.util.TreeSet<Integer> used = new java.util.TreeSet<>();
        private long s;

        Space(long seed) {
            this.s = seed;
        }

        private int next(int n) {
            s = (s * 0x5DEECE66DL + 0xBL) & ((1L << 48) - 1);
            return (int) ((s >>> 16) % n);
        }

        /// One fresh position whose spread hash satisfies
        /// `(spread & mask) == want`.
        BlockPos one(int mask, int want) {
            for (int guard = 0; guard < 10_000_000; guard++) {
                BlockPos p = new BlockPos(next(31) - 15, next(31), next(31) - 15);
                int h = spread(p);
                if ((h & mask) != want) {
                    continue;
                }
                if (used.add(h)) {
                    return p;
                }
            }
            throw new IllegalStateException("no position for mask " + mask + " want " + want);
        }

        List<BlockPos> many(int mask, int want, int count) {
            List<BlockPos> out = new ArrayList<>();
            for (int i = 0; i < count; i++) {
                out.add(one(mask, want));
            }
            return out;
        }

        /// `count` positions spread one per bucket over `[from, from + count)`
        /// of a sixty-four bucket table — enough entries to grow the table
        /// without piling any bucket up on the way.
        List<BlockPos> spreadOver(int from, int count) {
            List<BlockPos> out = new ArrayList<>();
            for (int i = 0; i < count; i++) {
                out.add(one(63, (from + i) % 64));
            }
            return out;
        }
    }

    /// An add.
    private static int[] add(BlockPos p) {
        return new int[] { 1, p.getX(), p.getY(), p.getZ() };
    }

    /// A remove.
    private static int[] del(BlockPos p) {
        return new int[] { 0, p.getX(), p.getY(), p.getZ() };
    }

    private static void addAll(List<int[]> ops, List<BlockPos> positions) {
        for (BlockPos p : positions) {
            ops.add(add(p));
        }
    }

    private static List<String> treeCases() {
        List<String> out = new ArrayList<>();

        // Twenty-five entries takes a sixteen-bucket table to sixty-four,
        // which is MIN_TREEIFY_CAPACITY: below it `treeifyBin` resizes
        // instead of treeifying, so nothing before this can make a tree. The
        // filler goes one per bucket so it makes no long chain of its own.
        //
        // A fresh `Space` per case, so a case's positions are guaranteed
        // tie-free among themselves and nothing leaks between cases.

        // Nine in one bucket of a sixty-four bucket table: the smallest thing
        // that treeifies at all. Eight would not — `putVal`'s binCount does
        // not count the head.
        Space space = new Space(11);
        List<int[]> ops = new ArrayList<>();
        addAll(ops, space.spreadOver(10, 25));
        addAll(ops, space.many(63, 5, 9));
        out.add(dumpOps("tree_min", ops));

        // Eight, which must still be a plain list. If a port treeifies at
        // eight this is the case that says so, and it says it in one element.
        space = new Space(11);
        ops = new ArrayList<>();
        addAll(ops, space.spreadOver(10, 25));
        addAll(ops, space.many(63, 5, 8));
        out.add(dumpOps("tree_eight_is_a_list", ops));

        // Twenty in one bucket: `putTreeVal` splices eleven entries in after
        // their tree parent instead of appending, and moves the root each
        // time.
        space = new Space(11);
        ops = new ArrayList<>();
        addAll(ops, space.spreadOver(10, 25));
        addAll(ops, space.many(63, 5, 20));
        out.add(dumpOps("tree_grow", ops));

        // Past the load threshold with a tree standing, so `TreeNode.split`
        // runs. These share a bucket at 64 but not necessarily at 128, so
        // both halves come out non-empty and each is either untreeified or
        // re-treeified.
        space = new Space(23);
        ops = new ArrayList<>();
        addAll(ops, space.spreadOver(10, 25));
        addAll(ops, space.many(63, 5, 20));
        addAll(ops, space.spreadOver(40, 20));
        out.add(dumpOps("tree_split_both_halves", ops));

        // The same, but the colliding set shares a bucket at 128 as well, so
        // the split puts everything on one side. `split` then leaves the bin
        // exactly as it was — the re-treeify is guarded on the *other* half
        // being non-empty, and a port that re-treeifies unconditionally moves
        // a root that vanilla left alone.
        space = new Space(23);
        ops = new ArrayList<>();
        addAll(ops, space.spreadOver(10, 25));
        addAll(ops, space.many(127, 5, 20));
        addAll(ops, space.spreadOver(40, 20));
        out.add(dumpOps("tree_split_one_side", ops));

        // A split whose halves are six or fewer, which untreeifies both back
        // to plain lists.
        space = new Space(37);
        ops = new ArrayList<>();
        addAll(ops, space.spreadOver(10, 25));
        addAll(ops, space.many(63, 5, 10));
        addAll(ops, space.spreadOver(40, 25));
        out.add(dumpOps("tree_untreeify_on_split", ops));

        // Removal from a tree: every other entry, then the rest. This reaches
        // `removeTreeNode`'s successor swap, `balanceDeletion`, and the
        // "the tree got short" untreeify.
        space = new Space(41);
        List<BlockPos> deep = space.many(63, 5, 20);
        ops = new ArrayList<>();
        addAll(ops, space.spreadOver(10, 25));
        addAll(ops, deep);
        for (int i = 0; i < deep.size(); i += 2) {
            ops.add(del(deep.get(i)));
        }
        for (int i = 1; i < deep.size(); i += 2) {
            ops.add(del(deep.get(i)));
        }
        out.add(dumpOps("tree_remove", ops));

        // Churn: adds and removes drawn from a pool that is two thirds
        // colliding, so bins treeify, untreeify, split and rebalance
        // repeatedly and out of any tidy order. Three streams, because one
        // stream that happens to avoid a branch proves nothing about it.
        for (int run = 0; run < 3; run++) {
            Space churn = new Space(101 + run);
            List<BlockPos> pool = new ArrayList<>();
            pool.addAll(churn.many(63, 5, 40));
            pool.addAll(churn.many(63, 37, 40));
            pool.addAll(churn.many(0, 0, 40));
            java.util.Random rng = new java.util.Random(7000 + run);
            ops = new ArrayList<>();
            for (int i = 0; i < 700; i++) {
                BlockPos p = pool.get(rng.nextInt(pool.size()));
                ops.add(rng.nextInt(10) < 7 ? add(p) : del(p));
            }
            out.add(dumpOps("tree_churn_" + run, ops));
        }

        // A conifer, because that is the shape this exists for: a spruce
        // carries enough foliage to trip the threshold where an oak does not.
        // Nothing here aims at a bucket — this is the real workload, and
        // whether it treeifies is the hashes' business.
        ops = new ArrayList<>();
        for (int y = 0; y < 30; y++) {
            ops.add(add(new BlockPos(0, 64 + y, 0)));
            ops.add(add(new BlockPos(1, 64 + y, 0)));
            ops.add(add(new BlockPos(0, 64 + y, 1)));
            ops.add(add(new BlockPos(1, 64 + y, 1)));
        }
        for (int y = 8; y < 30; y++) {
            int r = 4 - (y % 5);
            for (int dx = -r; dx <= r + 1; dx++) {
                for (int dz = -r; dz <= r + 1; dz++) {
                    if (Math.abs(dx) + Math.abs(dz) <= r + 1) {
                        ops.add(add(new BlockPos(dx, 64 + y, dz)));
                    }
                }
            }
        }
        out.add(dumpOps("conifer", ops));

        return out;
    }

    /// One operation-stream case: what was done, the order after every step
    /// as a fingerprint, and the full order at the end.
    private static String dumpOps(String name, List<int[]> ops) {
        HashSet<BlockPos> set = new HashSet<>();
        StringBuilder opText = new StringBuilder();
        StringBuilder fingerprints = new StringBuilder();
        for (int i = 0; i < ops.size(); i++) {
            int[] op = ops.get(i);
            BlockPos p = new BlockPos(op[1], op[2], op[3]);
            if (op[0] == 1) {
                set.add(p);
            } else {
                set.remove(p);
            }
            opText.append(i == 0 ? "" : ", ")
                .append("[").append(op[0]).append(", ").append(op[1]).append(", ")
                .append(op[2]).append(", ").append(op[3]).append("]");
            fingerprints.append(i == 0 ? "" : ", ").append(fingerprint(set));
        }
        StringBuilder order = new StringBuilder();
        int i = 0;
        for (BlockPos p : set) {
            order.append(i++ == 0 ? "" : ", ").append(pos(p));
        }
        return "{\"name\": \"" + name + "\", \"size\": " + set.size()
            + ", \"ops\": [" + opText + "]"
            + ", \"fingerprints\": [" + fingerprints + "]"
            + ", \"order\": [" + order + "]}";
    }

    /// An order-sensitive digest of the whole iteration order — FNV-1a over
    /// the coordinates, in the order the set hands them out. Two sets with
    /// the same members in a different order differ here.
    private static long fingerprint(HashSet<BlockPos> set) {
        long fp = 0xcbf29ce484222325L;
        for (BlockPos p : set) {
            fp = (fp ^ p.getX()) * 0x100000001b3L;
            fp = (fp ^ p.getY()) * 0x100000001b3L;
            fp = (fp ^ p.getZ()) * 0x100000001b3L;
        }
        return fp;
    }

    private static List<BlockPos> column(int x, int y, int z, int n) {
        List<BlockPos> out = new ArrayList<>();
        for (int i = 0; i < n; i++) {
            out.add(new BlockPos(x, y + i, z));
        }
        return out;
    }

    /// One case: what went in, and what came out.
    private static String dump(String name, List<BlockPos> inserted) {
        HashSet<BlockPos> set = new HashSet<>();
        for (BlockPos pos : inserted) {
            set.add(pos);
        }
        StringBuilder in = new StringBuilder();
        for (int i = 0; i < inserted.size(); i++) {
            in.append(i == 0 ? "" : ", ").append(pos(inserted.get(i)));
        }
        StringBuilder order = new StringBuilder();
        int i = 0;
        for (BlockPos pos : set) {
            order.append(i++ == 0 ? "" : ", ").append(pos(pos));
        }
        return "{\"name\": \"" + name + "\", \"size\": " + set.size()
            + ", \"inserted\": [" + in + "], \"order\": [" + order + "]}";
    }

    private static String pos(BlockPos p) {
        return "[" + p.getX() + ", " + p.getY() + ", " + p.getZ() + "]";
    }
}
