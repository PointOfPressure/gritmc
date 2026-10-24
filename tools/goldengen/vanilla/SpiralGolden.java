package net.minecraft.world.level.levelgen;

import java.io.PrintStream;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.server.Bootstrap;

/**
 * {@code BlockPos.spiralAround(centre, radius, first, second)}, captured.
 *
 * <p>This is the traversal {@code PortalForcer.createPortal} searches in, and
 * the reason it returns a <em>particular</em> portal rather than merely a
 * valid one: every candidate is scored by {@code distSqr} to the clicked
 * block and the smallest wins, so wherever two sites are equidistant — which
 * around a symmetric centre is most of them — the winner is whichever the
 * spiral reached first.
 *
 * <p>It is captured rather than transcribed-and-argued because the iterator
 * ({@code BlockPos$6}) has two properties a reading gets wrong:
 *
 * <ul>
 *   <li>the cursor is initialised <b>one step along {@code second}</b> and the
 *       first {@code computeNext} moves along {@code directions[(-1 + 4) % 4]}
 *       = {@code second.getOpposite()}, which undoes it — so the first
 *       position yielded is the centre, and a transcription that starts the
 *       cursor at the centre is off by one leg for its entire length;
 *   <li>{@code legSize} is {@code leg / 2 + 1} and is recomputed only when a
 *       leg ends, so the legs go 1, 1, 2, 2, 3, 3, … and the walk is a square
 *       spiral rather than a ring scan.
 * </ul>
 *
 * <p>Nothing here reads a world, a random source or a registry: the iterator
 * is arithmetic over {@code BlockPos} and {@code Direction}. Two runs of this
 * program must be byte-identical, and {@code ci/spiral-golden.sh} checks that
 * before it keeps the output.
 *
 * <p>Run by {@code ci/spiral-golden.sh}. Writes to
 * {@code crates/rustmc-server/golden/spiral_around.json}.
 */
public final class SpiralGolden {
    private SpiralGolden() {}

    public static void main(String[] args) {
        // Held BEFORE the bootstrap, which replaces System.out with a logger
        // that stamps every line with a wall-clock time — two runs would then
        // differ on every line and the identical-runs check would refuse a
        // capture that is in fact perfectly deterministic.
        PrintStream out = System.out;
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        out.println("{");
        out.println("  \"note\": \"BlockPos.spiralAround, vanilla's own iterator\",");
        out.println("  \"walks\": [");

        // Four walks, chosen so that no single bug passes all of them:
        //
        //   r=0            the degenerate case — legs = 0, so the walk is the
        //                  centre and nothing else. An off-by-one in the
        //                  `leg >= legs` end test yields more.
        //   r=1 EAST,SOUTH the smallest walk with corners, at the origin.
        //   r=2 EAST,SOUTH long enough that the leg lengths 1,1,2,2,3,3 are
        //                  all exercised, at a NEGATIVE, ASYMMETRIC centre so
        //                  a transcription that assumed the origin, or that
        //                  used truncating division on a negative coordinate,
        //                  comes out wrong.
        //   r=2 SOUTH,EAST the same radius with the two directions SWAPPED,
        //                  which is a different walk — without it, a
        //                  transcription that ignored its arguments and
        //                  hard-coded EAST/SOUTH would pass every other row.
        walk(out, new BlockPos(0, 64, 0), 0, Direction.EAST, Direction.SOUTH, true);
        walk(out, new BlockPos(0, 64, 0), 1, Direction.EAST, Direction.SOUTH, true);
        walk(out, new BlockPos(-13, 7, 41), 2, Direction.EAST, Direction.SOUTH, true);
        walk(out, new BlockPos(-13, 7, 41), 2, Direction.SOUTH, Direction.EAST, false);

        out.println("  ]");
        out.println("}");
    }

    private static void walk(
            PrintStream out, BlockPos centre, int radius, Direction a, Direction b, boolean comma) {
        StringBuilder sb = new StringBuilder();
        sb.append("    {\"centre\": [")
                .append(centre.getX()).append(", ")
                .append(centre.getY()).append(", ")
                .append(centre.getZ()).append("], \"radius\": ").append(radius)
                .append(", \"first\": \"").append(a.getName())
                .append("\", \"second\": \"").append(b.getName())
                .append("\", \"positions\": [");
        boolean first = true;
        for (BlockPos.MutableBlockPos p : BlockPos.spiralAround(centre, radius, a, b)) {
            if (!first) {
                sb.append(", ");
            }
            first = false;
            sb.append('[').append(p.getX()).append(',').append(p.getY()).append(',')
                    .append(p.getZ()).append(']');
        }
        sb.append("]}");
        if (comma) {
            sb.append(',');
        }
        out.println(sb);
    }
}
