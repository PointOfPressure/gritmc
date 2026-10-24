// BeardGolden — vanilla's `Beardifier`, dumped as a per-component golden.
//
// # Why this exists as components and not as one composed number
//
// The beardifier is the first term in this port with no byte-identical gate
// available: it is *meant* to change terrain, so "the world still matches"
// cannot be the test for it. What is left is a residual moving in a predicted
// direction — and a residual cannot say *which* of five pieces is wrong. A
// kernel that is off in the last bit and a switch arm wired to the wrong
// `TerrainAdjustment` both produce a plausible number.
//
// So every piece that has a vanilla-side oracle gets one here, and the
// composed world diff is checked last rather than instead:
//
//   kernel      the 13,824-entry `BEARD_KERNEL`, as raw float bits
//   beard       `getBeardContribution(int,int,int,int)` over a swept grid
//   bury        `getBuryContribution(double,double,double)` over a swept grid
//   compute     `Beardifier.compute` for a piece of each of the five
//               `TerrainAdjustment` values, and for a jigsaw junction
//
// # What this does NOT establish, stated so the composed diff cannot quietly
// # stand in for it
//
// `forStructuresInChunk` is **not** covered. It needs a `StructureManager`,
// which needs a server. So whether our port hands its beardifier the same
// pieces vanilla would — the same boxes, the same `groundLevelDelta`, the
// same rigid/non-rigid split, the same junction window — is a separate
// question with a separate oracle: the reference chunk's own
// `structures.starts.<id>.Children[].BB`. Nothing below tests it.
//
// # Bits, not decimals
//
// `BEARD_KERNEL` is `float[]` and `getBeardContribution` widens each entry to
// `double` at the point of use. Printing decimals would let a port that holds
// the kernel as `f64` agree to fifteen places and still be wrong in the last
// bit of every lookup — which is exactly the class of error this project
// keeps finding. Raw `floatToRawIntBits` / `doubleToRawLongBits` is the
// honest comparison.
//
// Converted to JSON by `ci/beard-golden.sh`.
//
// # The default package, deliberately
//
// `ci/beard-probe.sh` already proved this shape works: everything below is
// either public or reached by `setAccessible`, so nothing here needs to be
// *declared* inside `net.minecraft.world.level.levelgen`. Declaring it there
// would mean defining a class into a package the signed server jar already
// seals, which is a class of failure with nothing to do with the beard.
import net.minecraft.SharedConstants;
import net.minecraft.world.level.levelgen.Beardifier;
import net.minecraft.world.level.levelgen.DensityFunction;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.TerrainAdjustment;
import net.minecraft.world.level.levelgen.structure.pools.JigsawJunction;
import net.minecraft.world.level.levelgen.structure.pools.StructureTemplatePool;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.List;

public final class BeardGolden {

    /// The `dx`/`dz` sweep. Both are `Math.max(0, ..)` distances in `compute`,
    /// so they are never negative there — but the kernel guard is written for
    /// a signed index, and 12 is the exact point where `isInKernelRange`
    /// starts refusing. The values straddle it in both directions.
    private static final int[] DXZ = { 0, 1, 2, 3, 7, 11, 12, 13, 25 };

    /// The `dy` sweep. Unlike `dx`/`dz` this one really is signed: for
    /// `BEARD_THIN` it is `y - (box.minY + groundLevelDelta)`, which is
    /// negative for every point below the piece's ground. -13 and -12 straddle
    /// the low end of the guard the same way 11/12 straddle the high end.
    private static final int[] DY = { -25, -13, -12, -11, -5, 0, 5, 11, 12, 13 };

    /// `groundLevelDelta`-relative height, which enters as `dg + 0.5` and
    /// drives the sign of the whole term. 0 is the interesting one: it makes
    /// the half-block offset the entire numerator.
    private static final int[] DG = { -9, -1, 0, 1, 9 };

    /// The bury sweep. `getBuryContribution` is a `clampedMap` of the vector
    /// length over `0..6`, so the values bracket 6 exactly, and the halves
    /// are there because `compute` divides by two before calling it.
    private static final double[] BURY = { 0.0, 0.5, 1.0, 2.5, 3.0, 4.5, 5.9, 6.0, 6.1, 10.0 };

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        Field kernelField = Beardifier.class.getDeclaredField("BEARD_KERNEL");
        kernelField.setAccessible(true);
        float[] kernel = (float[]) kernelField.get(null);

        Method beard = Beardifier.class.getDeclaredMethod(
            "getBeardContribution", int.class, int.class, int.class, int.class);
        beard.setAccessible(true);
        Method bury = Beardifier.class.getDeclaredMethod(
            "getBuryContribution", double.class, double.class, double.class);
        bury.setAccessible(true);

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"source\": \"net.minecraft.world.level.levelgen.Beardifier\",\n");
        out.append("  \"kernel_radius\": ").append(Beardifier.BEARD_KERNEL_RADIUS).append(",\n");
        out.append("  \"kernel_len\": ").append(kernel.length).append(",\n");

        // The kernel, in its own storage order. The index arithmetic is
        // `getBeardContribution`'s — `kz*576 + kx*24 + ky` — and it is
        // deliberately NOT re-derived here: dumping the flat array means a
        // port that has the right values in the wrong order still fails.
        out.append("  \"kernel\": [");
        for (int i = 0; i < kernel.length; i++) {
            if (i > 0) {
                out.append(',');
            }
            if (i % 16 == 0) {
                out.append("\n    ");
            }
            out.append(Float.floatToRawIntBits(kernel[i]));
        }
        out.append("\n  ],\n");

        out.append("  \"beard\": [");
        boolean first = true;
        for (int dx : DXZ) {
            for (int dy : DY) {
                for (int dz : DXZ) {
                    for (int dg : DG) {
                        double v = (double) beard.invoke(null, dx, dy, dz, dg);
                        if (!first) {
                            out.append(',');
                        }
                        first = false;
                        out.append("\n    [").append(dx).append(',').append(dy).append(',')
                            .append(dz).append(',').append(dg).append(',')
                            .append(Double.doubleToRawLongBits(v)).append(']');
                    }
                }
            }
        }
        out.append("\n  ],\n");

        out.append("  \"bury\": [");
        first = true;
        for (double bx : BURY) {
            for (double by : BURY) {
                for (double bz : BURY) {
                    double v = (double) bury.invoke(null, bx, by, bz);
                    if (!first) {
                        out.append(',');
                    }
                    first = false;
                    out.append("\n    [").append(Double.doubleToRawLongBits(bx)).append(',')
                        .append(Double.doubleToRawLongBits(by)).append(',')
                        .append(Double.doubleToRawLongBits(bz)).append(',')
                        .append(Double.doubleToRawLongBits(v)).append(']');
                }
            }
        }
        out.append("\n  ],\n");

        // The composed term. One case per `TerrainAdjustment`, plus a
        // two-piece case (the sum is over pieces, and a port that returns
        // early on the first hit passes every single-piece case), plus a
        // junction-only case and a mixed one.
        //
        // The boxes are deliberately not cubes and not origin-aligned: a
        // 5x3x7 box at an offset catches an axis swap that a cube at the
        // origin cannot.
        BoundingBox box = new BoundingBox(20, 60, 30, 24, 62, 36);
        out.append("  \"compute\": [");
        first = true;
        for (TerrainAdjustment adjustment : TerrainAdjustment.values()) {
            for (int gld : new int[] { 0, 1, 4 }) {
                first = emitCase(out, first, adjustment.getSerializedName() + "/gld" + gld,
                    List.of(new Beardifier.Rigid(box, adjustment, gld)), List.of());
            }
        }
        first = emitCase(out, first, "two_pieces",
            List.of(
                new Beardifier.Rigid(box, TerrainAdjustment.BEARD_THIN, 1),
                new Beardifier.Rigid(new BoundingBox(26, 58, 33, 30, 64, 39),
                    TerrainAdjustment.BURY, 2)),
            List.of());
        first = emitCase(out, first, "junctions_only", List.of(),
            List.of(
                new JigsawJunction(22, 61, 33, 0, StructureTemplatePool.Projection.RIGID),
                new JigsawJunction(27, 59, 35, -2, StructureTemplatePool.Projection.TERRAIN_MATCHING)));
        first = emitCase(out, first, "mixed",
            List.of(new Beardifier.Rigid(box, TerrainAdjustment.BEARD_BOX, 3)),
            List.of(new JigsawJunction(21, 63, 31, 1, StructureTemplatePool.Projection.RIGID)));
        out.append("\n  ]\n");
        out.append("}\n");
        System.out.println(out);
    }

    /// One `Beardifier`, its affected box, and every sample in a range that
    /// reaches well outside the box on all six faces.
    ///
    /// The affected box is built the way `forStructuresInChunk` builds it:
    /// the union of the piece boxes and the junction source points,
    /// `inflatedBy(24)`. **24, not the kernel radius of 12** — that is
    /// vanilla's number at offset 413 and copying 12 there would make the
    /// probe disagree with vanilla in the shell between them.
    private static boolean emitCase(StringBuilder out, boolean first, String name,
            List<Beardifier.Rigid> pieces, List<JigsawJunction> junctions) {
        BoundingBox affected = null;
        for (Beardifier.Rigid piece : pieces) {
            affected = include(affected, piece.box());
        }
        for (JigsawJunction junction : junctions) {
            affected = include(affected, new BoundingBox(
                junction.getSourceX(), junction.getSourceGroundY(), junction.getSourceZ(),
                junction.getSourceX(), junction.getSourceGroundY(), junction.getSourceZ()));
        }
        BoundingBox inflated = affected == null ? null : affected.inflatedBy(24);
        Beardifier beardifier = new Beardifier(pieces, junctions, inflated);

        if (!first) {
            out.append(',');
        }
        out.append("\n    {\n      \"name\": \"").append(name).append("\",\n");
        out.append("      \"pieces\": [");
        for (int i = 0; i < pieces.size(); i++) {
            Beardifier.Rigid piece = pieces.get(i);
            BoundingBox b = piece.box();
            out.append(i > 0 ? "," : "").append("[")
                .append(b.minX()).append(',').append(b.minY()).append(',').append(b.minZ())
                .append(',').append(b.maxX()).append(',').append(b.maxY()).append(',')
                .append(b.maxZ()).append(",\"")
                .append(piece.terrainAdjustment().getSerializedName()).append("\",")
                .append(piece.groundLevelDelta()).append(']');
        }
        out.append("],\n      \"junctions\": [");
        for (int i = 0; i < junctions.size(); i++) {
            JigsawJunction junction = junctions.get(i);
            out.append(i > 0 ? "," : "").append("[")
                .append(junction.getSourceX()).append(',')
                .append(junction.getSourceGroundY()).append(',')
                .append(junction.getSourceZ()).append(']');
        }
        out.append("],\n      \"affected\": ");
        if (inflated == null) {
            out.append("null");
        } else {
            out.append('[').append(inflated.minX()).append(',').append(inflated.minY())
                .append(',').append(inflated.minZ()).append(',').append(inflated.maxX())
                .append(',').append(inflated.maxY()).append(',').append(inflated.maxZ())
                .append(']');
        }
        out.append(",\n      \"samples\": [");
        boolean firstSample = true;
        for (int x = 8; x <= 40; x += 2) {
            for (int y = 48; y <= 76; y += 2) {
                for (int z = 18; z <= 50; z += 2) {
                    double v = beardifier.compute(
                        new DensityFunction.SinglePointContext(x, y, z));
                    if (!firstSample) {
                        out.append(',');
                    }
                    firstSample = false;
                    out.append("\n        [").append(x).append(',').append(y).append(',')
                        .append(z).append(',').append(Double.doubleToRawLongBits(v))
                        .append(']');
                }
            }
        }
        out.append("\n      ]\n    }");
        return false;
    }

    /// `Beardifier.includeBoundingBox` — null-tolerant union.
    private static BoundingBox include(BoundingBox current, BoundingBox next) {
        return current == null ? next : BoundingBox.encapsulating(current, next);
    }

    private BeardGolden() {
    }
}
