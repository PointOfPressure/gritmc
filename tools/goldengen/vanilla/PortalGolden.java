package net.minecraft.world.level.levelgen;

import java.io.PrintStream;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.MappedRegistry;
import net.minecraft.core.registries.Registries;
import net.minecraft.tags.TagLoader;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.tags.BlockTags;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.portal.PortalShape;

/**
 * `PortalShape.findAnyShape` over a set of hand-built obsidian frames.
 *
 * <p>The whole algorithm is a walk over a {@link BlockGetter}, so it can be
 * driven against a map instead of a world: {@link Grid} below is fifteen lines
 * and answers the five abstract methods `BlockGetter` and
 * `LevelHeightAccessor` declare. Nothing about the search is stubbed — this is
 * vanilla's own `findAnyShape`, and what it reads is a real
 * `Blocks.OBSIDIAN.defaultBlockState()`.
 *
 * <p>What the scenarios are chosen to separate:
 *
 * <ul>
 *   <li>the four size bounds, on both sides — 2..21 wide, 3..21 tall — because
 *       a transcription with `&lt;` for `&lt;=` passes every interior case;
 *   <li>**the corners, which vanilla does not check.** A frame missing all
 *       four corner blocks is still a valid portal, and a transcription that
 *       walks the perimeter instead of the two sides and the top refuses it;
 *   <li>`numPortalBlocks`, counted during the height walk and only for blocks
 *       that are already `minecraft:nether_portal`, so `isComplete` separates a
 *       lit portal from an unlit frame of the same shape;
 *   <li>the downward search in `calculateBottomLeft`, which starts from the
 *       clicked block and walks down through empty blocks — capped at 21 below
 *       it or the world floor, whichever is higher.
 * </ul>
 *
 * <p>Run by {@code ci/portal-golden.sh}. Writes to
 * {@code crates/rustmc-server/golden/portal_shape.json}.
 */
public final class PortalGolden {
    private PortalGolden() {}

    /** An in-memory `BlockGetter`. Anything unset is air. */
    private static final class Grid implements BlockGetter {
        private final Map<BlockPos, BlockState> blocks = new HashMap<>();

        void put(int x, int y, int z, BlockState s) {
            blocks.put(new BlockPos(x, y, z), s);
        }

        @Override
        public BlockState getBlockState(BlockPos p) {
            return blocks.getOrDefault(p, Blocks.AIR.defaultBlockState());
        }

        @Override
        public BlockEntity getBlockEntity(BlockPos p) {
            return null;
        }

        @Override
        public FluidState getFluidState(BlockPos p) {
            return getBlockState(p).getFluidState();
        }

        @Override
        public int getHeight() {
            return 384;
        }

        @Override
        public int getMinY() {
            return -64;
        }
    }

    public static void main(String[] args) {
        PrintStream out = System.out;
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // `Bootstrap.bootStrap()` registers the blocks, freezes the registry
        // and leaves every block TAG unbound, so `BlockState.is(BlockTags.FIRE)`
        // is false for `minecraft:fire` in this JVM and `PortalShape.isEmpty`
        // answers differently from a real server. That is not a detail to work
        // around: `isEmpty` is one of the three predicates the whole search is
        // built on, and a capture taken without it is a capture of a different
        // algorithm.
        //
        // `bindTags` is refused on a frozen registry ("Registry is already
        // frozen"), so the tag goes in the way a datapack reload does —
        // `prepareTagReload(...).apply()`. The membership is the datapack
        // extract's own `data/minecraft/tags/block/fire.json`, which is
        // `minecraft:fire` and `minecraft:soul_fire`; `ci/portal-golden.sh`
        // refuses the capture if the fire row comes back looking unbound, so a
        // JVM where this stops working cannot produce a green file.
        ((MappedRegistry<net.minecraft.world.level.block.Block>) BuiltInRegistries.BLOCK)
                .prepareTagReload(new TagLoader.LoadResult<>(
                        Registries.BLOCK,
                        Map.of(
                                BlockTags.FIRE,
                                List.of(
                                        BuiltInRegistries.BLOCK.wrapAsHolder(Blocks.FIRE),
                                        BuiltInRegistries.BLOCK.wrapAsHolder(Blocks.SOUL_FIRE)))))
                .apply();

        BlockState obsidian = Blocks.OBSIDIAN.defaultBlockState();
        BlockState portal = Blocks.NETHER_PORTAL.defaultBlockState();
        BlockState fire = Blocks.FIRE.defaultBlockState();
        BlockState stone = Blocks.STONE.defaultBlockState();

        List<String> rows = new ArrayList<>();

        // The standard 2x3 doorway on the X axis: interior x in 0..1, y in
        // 1..3, at z = 0.
        rows.add(row("min-2x3-x", frame(obsidian, 2, 3, Direction.Axis.X), Direction.Axis.X, 0, 1, 0));
        // The same frame with its interior already lit.
        Grid lit = frame(obsidian, 2, 3, Direction.Axis.X);
        fill(lit, 2, 3, Direction.Axis.X, portal);
        rows.add(row("min-2x3-x-lit", lit, Direction.Axis.X, 0, 1, 0));
        // Half-lit: valid, not complete.
        Grid half = frame(obsidian, 2, 3, Direction.Axis.X);
        half.put(0, 1, 0, portal);
        half.put(1, 1, 0, portal);
        rows.add(row("min-2x3-x-half-lit", half, Direction.Axis.X, 0, 1, 0));

        // The same on Z, where `rightDir` is SOUTH rather than WEST.
        rows.add(row("min-2x3-z", frame(obsidian, 2, 3, Direction.Axis.Z), Direction.Axis.Z, 0, 1, 0));

        // The four bounds, each from the wrong side.
        rows.add(row("too-narrow-1x3", frame(obsidian, 1, 3, Direction.Axis.X), Direction.Axis.X, 0, 1, 0));
        rows.add(row("too-short-2x2", frame(obsidian, 2, 2, Direction.Axis.X), Direction.Axis.X, 0, 1, 0));
        rows.add(row("max-21x21", frame(obsidian, 21, 21, Direction.Axis.X), Direction.Axis.X, 0, 1, 0));
        rows.add(row("too-wide-22x3", frame(obsidian, 22, 3, Direction.Axis.X), Direction.Axis.X, 0, 1, 0));
        rows.add(row("too-tall-2x22", frame(obsidian, 2, 22, Direction.Axis.X), Direction.Axis.X, 0, 1, 0));

        // Corners removed. Vanilla walks the two sides and the top and never
        // looks at (-1, 0) or (width, height); this frame is valid.
        Grid noCorners = frame(obsidian, 3, 4, Direction.Axis.X);
        noCorners.put(-1, 0, 0, Blocks.AIR.defaultBlockState());
        noCorners.put(3, 0, 0, Blocks.AIR.defaultBlockState());
        noCorners.put(-1, 5, 0, Blocks.AIR.defaultBlockState());
        noCorners.put(3, 5, 0, Blocks.AIR.defaultBlockState());
        rows.add(row("no-corners-3x4", noCorners, Direction.Axis.X, 0, 1, 0));

        // A hole in the top edge: invalid, and the hole is in the middle so a
        // check that only tests the ends would miss it.
        Grid holeTop = frame(obsidian, 3, 4, Direction.Axis.X);
        holeTop.put(1, 5, 0, Blocks.AIR.defaultBlockState());
        rows.add(row("hole-in-top-3x4", holeTop, Direction.Axis.X, 0, 1, 0));

        // A hole in a side: the height walk stops there.
        Grid holeSide = frame(obsidian, 3, 4, Direction.Axis.X);
        holeSide.put(-1, 3, 0, Blocks.AIR.defaultBlockState());
        rows.add(row("hole-in-side-3x4", holeSide, Direction.Axis.X, 0, 1, 0));

        // The wrong material. Only OBSIDIAN is FRAME.
        rows.add(row("stone-frame-2x3", frame(stone, 2, 3, Direction.Axis.X), Direction.Axis.X, 0, 1, 0));

        // Fire inside the frame. `isEmpty` accepts `BlockTags.FIRE`, so this
        // is still a valid shape — and it is the one row here whose answer
        // depends on a *tag* being bound, which is why the shell script
        // asserts it separately.
        Grid withFire = frame(obsidian, 2, 3, Direction.Axis.X);
        withFire.put(0, 1, 0, fire);
        rows.add(row("fire-inside-2x3", withFire, Direction.Axis.X, 0, 1, 0));

        // Stone inside the frame: not empty, so the width walk stops at 0.
        Grid withStone = frame(obsidian, 2, 3, Direction.Axis.X);
        withStone.put(1, 1, 0, stone);
        rows.add(row("stone-inside-2x3", withStone, Direction.Axis.X, 0, 1, 0));

        // Entered from the TOP of the interior, so `calculateBottomLeft` has
        // to walk down two blocks before the width walk starts.
        rows.add(row("entered-from-top", frame(obsidian, 2, 3, Direction.Axis.X), Direction.Axis.X, 0, 3, 0));
        // Entered from the right-hand column, so the leftward walk has to move.
        rows.add(row("entered-from-right", frame(obsidian, 4, 3, Direction.Axis.X), Direction.Axis.X, 3, 1, 0));

        // Nothing at all: every field zero, and `bottomLeft` is the position
        // handed in rather than null.
        rows.add(row("empty-world", new Grid(), Direction.Axis.X, 0, 1, 0));

        out.println("{");
        out.println("  \"source\": \"net.minecraft.world.level.portal.PortalShape.findAnyShape, 26.2\",");
        out.println("  \"minWidth\": 2, \"maxWidth\": " + PortalShape.MAX_WIDTH + ",");
        out.println("  \"minHeight\": 3, \"maxHeight\": " + PortalShape.MAX_HEIGHT + ",");
        out.println("  \"shapes\": [");
        for (int i = 0; i < rows.size(); i++) {
            out.print("    " + rows.get(i));
            out.println(i + 1 < rows.size() ? "," : "");
        }
        out.println("  ]");
        out.println("}");
    }

    /**
     * An obsidian frame `w` wide and `h` tall whose interior starts at
     * (0, 1, 0) and runs along `axis`. The frame occupies the ring around it,
     * corners included.
     */
    private static Grid frame(BlockState mat, int w, int h, Direction.Axis axis) {
        Grid g = new Grid();
        for (int u = -1; u <= w; u++) {
            put(g, axis, u, 0, mat); // floor, corners included
            put(g, axis, u, h + 1, mat); // ceiling
        }
        for (int v = 1; v <= h; v++) {
            put(g, axis, -1, v, mat);
            put(g, axis, w, v, mat);
        }
        return g;
    }

    private static void fill(Grid g, int w, int h, Direction.Axis axis, BlockState s) {
        for (int u = 0; u < w; u++) {
            for (int v = 1; v <= h; v++) {
                put(g, axis, u, v, s);
            }
        }
    }

    /** `u` runs along the portal's own axis, `v` is world y, the third is 0. */
    private static void put(Grid g, Direction.Axis axis, int u, int v, BlockState s) {
        if (axis == Direction.Axis.X) {
            g.put(u, v, 0, s);
        } else {
            g.put(0, v, u, s);
        }
    }

    private static String row(String name, Grid g, Direction.Axis axis, int u, int y, int unusedZ) {
        BlockPos from = axis == Direction.Axis.X ? new BlockPos(u, y, 0) : new BlockPos(0, y, u);
        PortalShape s = PortalShape.findAnyShape(g, from, axis);
        BlockPos bl = bottomLeft(s);
        return String.format(
                "{\"name\": \"%s\", \"axis\": \"%s\", \"from\": [%d, %d, %d],"
                        + " \"width\": %d, \"height\": %d, \"numPortalBlocks\": %d,"
                        + " \"bottomLeft\": [%d, %d, %d], \"rightDir\": \"%s\","
                        + " \"valid\": %b, \"complete\": %b, \"portalPositions\": %s}",
                name,
                axis.getSerializedName(),
                from.getX(),
                from.getY(),
                from.getZ(),
                width(s),
                height(s),
                numPortalBlocks(s),
                bl.getX(),
                bl.getY(),
                bl.getZ(),
                rightDir(s).getSerializedName(),
                s.isValid(),
                s.isComplete(),
                portalPositions(s));
    }

    /**
     * The positions `createPortalBlocks` writes, in its own order.
     *
     * <p>It is `BlockPos.betweenClosed(bottomLeft, bottomLeft.above(height-1)
     * .relative(rightDir, width-1))`, which normalises the two corners
     * component-wise and then iterates **x fastest, then y, then z** — an
     * axis-aligned box walk in world coordinates, not a column-by-column walk
     * along `rightDir`. For a WEST-facing portal that reverses the horizontal
     * order relative to the search; for a SOUTH-facing one it does not. Every
     * one of these is a block update a client sees, so the order is part of
     * the behaviour and is captured rather than assumed.
     */
    private static String portalPositions(PortalShape s) {
        if (!s.isValid()) {
            return "[]";
        }
        BlockPos bl = bottomLeft(s);
        BlockPos far = bl.above(height(s) - 1).relative(rightDir(s), width(s) - 1);
        StringBuilder sb = new StringBuilder("[");
        boolean first = true;
        for (BlockPos p : BlockPos.betweenClosed(bl, far)) {
            if (!first) {
                sb.append(", ");
            }
            first = false;
            sb.append(String.format("[%d, %d, %d]", p.getX(), p.getY(), p.getZ()));
        }
        return sb.append("]").toString();
    }

    // The six fields are private and have no accessors, so they are read
    // reflectively. Reading them is the point: `width`, `height` and
    // `numPortalBlocks` are what `isValid` and `isComplete` are computed from,
    // and a capture of only the two booleans would agree with a transcription
    // that got the numbers wrong in compensating ways.
    private static int width(PortalShape s) {
        return intField(s, "width");
    }

    private static int height(PortalShape s) {
        return intField(s, "height");
    }

    private static int numPortalBlocks(PortalShape s) {
        return intField(s, "numPortalBlocks");
    }

    private static int intField(PortalShape s, String name) {
        try {
            var f = PortalShape.class.getDeclaredField(name);
            f.setAccessible(true);
            return f.getInt(s);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("PortalShape." + name + " is gone", e);
        }
    }

    private static BlockPos bottomLeft(PortalShape s) {
        try {
            var f = PortalShape.class.getDeclaredField("bottomLeft");
            f.setAccessible(true);
            return (BlockPos) f.get(s);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("PortalShape.bottomLeft is gone", e);
        }
    }

    private static Direction rightDir(PortalShape s) {
        try {
            var f = PortalShape.class.getDeclaredField("rightDir");
            f.setAccessible(true);
            return (Direction) f.get(s);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("PortalShape.rightDir is gone", e);
        }
    }

}
