// MobMathGolden — the Mth methods the mob AI's head-turn and stroll code
// calls, as raw bits.
//
// `Mth.atan2` is a table approximation, not `Math.atan2`, and its two
// tables are built with `Math.asin`/`Math.cos` — fdlibm on the JVM, the
// platform libm in Rust. Whether the 257 entries agree to the bit is the
// first question this file answers; the second is whether the port's
// `atan2` then agrees on a spread of inputs, which it can only do if the
// tables do and the arithmetic order is the jar's.
//
// What it dumps, every value as raw bits:
//
//   asin_tab, cos_tab   the two 257-entry tables, read back through
//                       Mth.atan2 itself is impossible, so they are
//                       rebuilt here exactly as Mth.<clinit> does and the
//                       rebuild is what is compared. If the JVM's
//                       Math.asin differed from its own StrictMath the
//                       tables would still be what the jar uses, because
//                       the jar calls Math.asin too.
//   atan2               2000 (y, x) pairs from java.util.Random, both
//                       signs, magnitudes to 1e3, plus the axes.
//   wrap                wrapDegrees(float) on 500 floats in [-1000, 1000].
//   rotate              rotateIfNecessary(a, b, c) on random floats.
//   approach            approachDegrees(a, b, c) on random floats.
//   diff                degreesDifference(a, b).
//   ceil_div            positiveCeilDiv(n, 2) for n in -10..40.
//   sqrt_f              Mth.sqrt(float) on the distances Node computes.
//
// Converted to JSON by `ci/mob-math-golden.sh`.

package rustmc.golden;

import java.util.Random;
import net.minecraft.util.Mth;

public final class MobMathGolden {

    public static void main(String[] args) {
        StringBuilder out = new StringBuilder("{\n  \"rng\": \"mob_math\",\n");

        // The tables, rebuilt as Mth.<clinit> builds them.
        out.append("  \"asin_tab\": [");
        StringBuilder cosTab = new StringBuilder();
        for (int i = 0; i < 257; i++) {
            double d = (double) i / 256.0;
            double asin = Math.asin(d);
            out.append(i == 0 ? "" : ", ").append(Double.doubleToRawLongBits(asin));
            cosTab.append(i == 0 ? "" : ", ").append(Double.doubleToRawLongBits(Math.cos(asin)));
        }
        out.append("],\n  \"cos_tab\": [").append(cosTab).append("],\n");
        out.append("  \"frac_bias\": ").append(Double.doubleToRawLongBits(Double.longBitsToDouble(4805340802404319232L))).append(",\n");

        Random rng = new Random(20260904L);
        out.append("  \"atan2\": [\n");
        double[][] fixed = {{0, 1}, {1, 0}, {0, -1}, {-1, 0}, {1, 1}, {-1, 1}, {1, -1}, {-1, -1},
                            {0, 0}, {1e-9, 1}, {1, 1e-9}, {3, 4}, {-3, 4}, {0.5, 0.5}};
        int n = 0;
        for (double[] p : fixed) {
            out.append(n++ == 0 ? "    " : ",\n    ")
               .append("[").append(Double.doubleToRawLongBits(p[0])).append(", ")
               .append(Double.doubleToRawLongBits(p[1])).append(", ")
               .append(Double.doubleToRawLongBits(Mth.atan2(p[0], p[1]))).append("]");
        }
        for (int i = 0; i < 2000; i++) {
            double scale = Math.pow(10.0, rng.nextInt(7) - 3);
            double y = (rng.nextDouble() * 2.0 - 1.0) * scale;
            double x = (rng.nextDouble() * 2.0 - 1.0) * scale;
            out.append(",\n    [").append(Double.doubleToRawLongBits(y)).append(", ")
               .append(Double.doubleToRawLongBits(x)).append(", ")
               .append(Double.doubleToRawLongBits(Mth.atan2(y, x))).append("]");
        }
        out.append("\n  ],\n");

        out.append("  \"wrap\": [");
        for (int i = 0; i < 500; i++) {
            float f = (rng.nextFloat() * 2000.0f) - 1000.0f;
            if (i % 50 == 0) f = 180.0f * (i / 50 - 5);
            out.append(i == 0 ? "" : ", ").append("[").append(Float.floatToRawIntBits(f)).append(", ")
               .append(Float.floatToRawIntBits(Mth.wrapDegrees(f))).append("]");
        }
        out.append("],\n");

        out.append("  \"rotate\": [");
        for (int i = 0; i < 300; i++) {
            float a = (rng.nextFloat() * 720.0f) - 360.0f;
            float b = (rng.nextFloat() * 720.0f) - 360.0f;
            float c = rng.nextFloat() * 90.0f;
            out.append(i == 0 ? "" : ", ").append("[").append(Float.floatToRawIntBits(a)).append(", ")
               .append(Float.floatToRawIntBits(b)).append(", ").append(Float.floatToRawIntBits(c)).append(", ")
               .append(Float.floatToRawIntBits(Mth.rotateIfNecessary(a, b, c))).append("]");
        }
        out.append("],\n");

        out.append("  \"approach\": [");
        for (int i = 0; i < 300; i++) {
            float a = (rng.nextFloat() * 720.0f) - 360.0f;
            float b = (rng.nextFloat() * 720.0f) - 360.0f;
            float c = (rng.nextFloat() * 90.0f) - 45.0f;
            out.append(i == 0 ? "" : ", ").append("[").append(Float.floatToRawIntBits(a)).append(", ")
               .append(Float.floatToRawIntBits(b)).append(", ").append(Float.floatToRawIntBits(c)).append(", ")
               .append(Float.floatToRawIntBits(Mth.approachDegrees(a, b, c))).append("]");
        }
        out.append("],\n");

        out.append("  \"diff\": [");
        for (int i = 0; i < 300; i++) {
            float a = (rng.nextFloat() * 720.0f) - 360.0f;
            float b = (rng.nextFloat() * 720.0f) - 360.0f;
            out.append(i == 0 ? "" : ", ").append("[").append(Float.floatToRawIntBits(a)).append(", ")
               .append(Float.floatToRawIntBits(b)).append(", ")
               .append(Float.floatToRawIntBits(Mth.degreesDifference(a, b))).append("]");
        }
        out.append("],\n");

        out.append("  \"ceil_div\": [");
        for (int i = -10; i <= 40; i++) {
            out.append(i == -10 ? "" : ", ").append("[").append(i).append(", ").append(Mth.positiveCeilDiv(i, 2)).append("]");
        }
        out.append("],\n");

        out.append("  \"sqrt_f\": [");
        for (int i = 0; i < 300; i++) {
            float f = (float) rng.nextInt(2000) + (i % 3 == 0 ? 0.0f : rng.nextFloat());
            out.append(i == 0 ? "" : ", ").append("[").append(Float.floatToRawIntBits(f)).append(", ")
               .append(Float.floatToRawIntBits(Mth.sqrt(f))).append("]");
        }
        out.append("]\n}\n");
        System.out.print(out);
    }
}
