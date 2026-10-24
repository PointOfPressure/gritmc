// FancyMathGolden — the platform transcendentals the fancy tree placer calls.
//
// `FancyTrunkPlacer` is the only worldgen code that reaches for `Math.sin`,
// `Math.cos`, `Math.pow` and `Math.sqrt` on values it computed itself, and
// then **floors** the result into a block coordinate. ORACLE.md 5c already
// measured `Math.sin`/`cos` against Rust at the angles the *ore* feature
// produces — 22 of 4097 differed by one ulp, none of which moved a floored
// coordinate. The fancy placer's angles are a different set and its
// multipliers are larger, so the same conclusion has to be re-earned rather
// than assumed.
//
// What it dumps, as raw bits so nothing is lost to decimal formatting:
//
//   sin/cos  at `nextFloat() * 2 * PI`, the branch bearing
//   pow      at `Math.pow(height / 13.0, 2.0)`, the branch-count curve
//   sqrt     at `Math.sqrt(dx*dx + dz*dz)`, the branch slope
//
// The angles are the ones a real stream produces: a `LegacyRandomSource`
// walked for as many floats as a chunk's worth of fancy oaks would take, not
// a uniform sweep, because the question is about the values that actually
// occur.
//
// Converted to JSON by `ci/fancy-math-golden.sh`.

package net.minecraft.world.level.levelgen;

public final class FancyMathGolden {

    /// One value per line, as bits, so the comparison is exact.
    public static void main(String[] args) {
        StringBuilder out = new StringBuilder("{\n  \"rng\": \"fancy_math\",\n");

        // The bearings. `FancyTrunkPlacer` computes
        //   angle  = random.nextFloat() * 2.0f * PI
        //   length = 1.0 * shape * (random.nextFloat() + 0.328)
        //   x      = length * sin(angle) + 0.5
        //   z      = length * cos(angle) + 0.5
        // so the argument is a float promoted to double, which is a much
        // coarser grid than an arbitrary double and is the whole reason this
        // is worth measuring separately.
        out.append("  \"trig\": [\n");
        java.util.Random source = new java.util.Random(20260725L);
        StringBuilder rows = new StringBuilder();
        int n = 0;
        for (int i = 0; i < 4096; i++) {
            float f = source.nextFloat();
            double angle = (double) (f * 2.0f * (float) Math.PI);
            rows.append(n++ == 0 ? "" : ",\n")
                .append("    {\"bits\": ").append(Double.doubleToRawLongBits(angle))
                .append(", \"sin\": ").append(Double.doubleToRawLongBits(Math.sin(angle)))
                .append(", \"cos\": ").append(Double.doubleToRawLongBits(Math.cos(angle)))
                .append("}");
        }
        out.append(rows).append("\n  ],\n");

        // `Math.pow(1.0 * height / 13.0, 2.0)` for every tree height the
        // placer can see. `base_height + rand_a + rand_b` tops out at 32 + 24
        // + 24 in the codec's ranges, and the placer adds two.
        out.append("  \"pow\": [\n");
        rows = new StringBuilder();
        n = 0;
        for (int height = 0; height <= 82; height++) {
            double x = 1.0 * height / 13.0;
            rows.append(n++ == 0 ? "" : ",\n")
                .append("    {\"height\": ").append(height)
                .append(", \"x\": ").append(Double.doubleToRawLongBits(x))
                .append(", \"pow\": ").append(Double.doubleToRawLongBits(Math.pow(x, 2.0)))
                .append("}");
        }
        out.append(rows).append("\n  ],\n");

        // `Math.sqrt(dx * dx + dz * dz)` over the offsets a branch can have.
        // IEEE 754 requires sqrt to be correctly rounded, so this one *should*
        // be exact everywhere — which is worth a dump precisely because it is
        // the assumption the others are being compared against.
        out.append("  \"sqrt\": [\n");
        rows = new StringBuilder();
        n = 0;
        for (int dx = -16; dx <= 16; dx++) {
            for (int dz = -16; dz <= 16; dz++) {
                double v = (double) (dx * dx + dz * dz);
                rows.append(n++ == 0 ? "" : ",\n")
                    .append("    {\"v\": ").append(Double.doubleToRawLongBits(v))
                    .append(", \"sqrt\": ").append(Double.doubleToRawLongBits(Math.sqrt(v)))
                    .append("}");
            }
        }
        out.append(rows).append("\n  ]\n}\n");
        System.out.print(out);
    }
}
