// BeardProbe — vanilla's own `Beardifier`, as an oracle for each component.
//
// # Why component oracles and not just the composed diff
//
// The beard is the first change in this sequence with no byte-identical gate:
// it is meant to change terrain, so "the world still matches" cannot be the
// test. What is left is a residual moving in a predicted direction, and a
// residual cannot say *which* of five pieces is wrong — a subtly wrong kernel
// and a subtly wrong switch arm both produce a plausible number.
//
// So each piece gets its own oracle here, and the composed result is checked
// last rather than instead:
//
//   kernel    the 13,824-entry `BEARD_KERNEL`, as raw float bits
//   compute   `Beardifier.compute` at a range of block positions
//
// `Beardifier(List<Rigid>, List<JigsawJunction>, BoundingBox)` is public, so
// this needs no `StructureManager` and no server. `BEARD_KERNEL` is private
// static and comes out by `setAccessible`, the same way `GoldenDump` already
// reads `DensityFunctions$Marker`.
//
// # Reading the kernel as bits, not as decimals
//
// `BEARD_KERNEL` is `float[]`, and `getBeardContribution` widens each entry to
// `double` at the point of use. Printing decimals would let a port that stores
// the kernel as `f64` agree to fifteen places and still be wrong in the last
// bit of every lookup, which is exactly the class of error this project keeps
// finding. The raw `Float.floatToRawIntBits` is the honest comparison.
//
// # What this probe does NOT establish
//
// It answers for the pieces and junctions it is *given*. Whether our port
// hands the same pieces to its beardifier — the same boxes, the same
// `groundLevelDelta`, the same `TerrainAdjustment` — is a different question
// with a different oracle: the reference chunk's own
// `structures.starts.<id>.Children[].BB`. Saying so explicitly because the
// composed diff would otherwise be quietly standing in for it.
import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.levelgen.Beardifier;
import net.minecraft.world.level.levelgen.DensityFunction;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.TerrainAdjustment;
import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

public final class BeardProbe {

    private static void usage() {
        System.err.println("usage: BeardProbe kernel");
        System.err.println("       BeardProbe compute <x0> <y0> <z0> <x1> <y1> <z1> <piece>...");
        System.err.println("  piece = x0,y0,z0,x1,y1,z1,ADJUSTMENT[,groundLevelDelta]");
        System.exit(2);
    }

    public static void main(String[] args) throws Exception {
        if (args.length < 1) {
            usage();
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        if ("kernel".equals(args[0])) {
            Field f = Beardifier.class.getDeclaredField("BEARD_KERNEL");
            f.setAccessible(true);
            float[] kernel = (float[]) f.get(null);
            System.out.println("KERNEL len=" + kernel.length
                + " radius=" + Beardifier.BEARD_KERNEL_RADIUS);
            for (int i = 0; i < kernel.length; i++) {
                System.out.println("K " + i + " " + Float.floatToRawIntBits(kernel[i]));
            }
            return;
        }

        if (!"compute".equals(args[0]) || args.length < 8) {
            usage();
        }
        int x0 = Integer.parseInt(args[1]);
        int y0 = Integer.parseInt(args[2]);
        int z0 = Integer.parseInt(args[3]);
        int x1 = Integer.parseInt(args[4]);
        int y1 = Integer.parseInt(args[5]);
        int z1 = Integer.parseInt(args[6]);

        List<Beardifier.Rigid> pieces = new ArrayList<>();
        BoundingBox affected = null;
        for (int i = 7; i < args.length; i++) {
            String[] p = args[i].split(",");
            if (p.length < 7) {
                usage();
            }
            BoundingBox box = new BoundingBox(
                Integer.parseInt(p[0].trim()), Integer.parseInt(p[1].trim()),
                Integer.parseInt(p[2].trim()), Integer.parseInt(p[3].trim()),
                Integer.parseInt(p[4].trim()), Integer.parseInt(p[5].trim()));
            TerrainAdjustment adjustment =
                TerrainAdjustment.valueOf(p[6].trim().toUpperCase(Locale.ROOT));
            int groundLevelDelta = p.length > 7 ? Integer.parseInt(p[7].trim()) : 0;
            pieces.add(new Beardifier.Rigid(box, adjustment, groundLevelDelta));
            System.out.println("PIECE " + box + " " + adjustment
                + " groundLevelDelta=" + groundLevelDelta);
            // `includeBoundingBox` is what `forStructuresInChunk` uses to grow
            // the affected box; inflating each piece by the kernel radius is
            // the same bound and needs no private access.
            BoundingBox grown = box.inflatedBy(Beardifier.BEARD_KERNEL_RADIUS);
            affected = affected == null ? grown : BoundingBox.encapsulatingBoxes(
                List.of(affected, grown)).orElseThrow();
        }

        Beardifier beardifier = new Beardifier(pieces, List.of(), affected);
        System.out.println("AFFECTED " + affected);
        for (int x = x0; x <= x1; x++) {
            for (int y = y0; y <= y1; y++) {
                for (int z = z0; z <= z1; z++) {
                    double v = beardifier.compute(
                        new DensityFunction.SinglePointContext(x, y, z));
                    System.out.println("BEARD " + x + " " + y + " " + z
                        + " " + Double.doubleToRawLongBits(v) + " " + v);
                }
            }
        }
    }
}
