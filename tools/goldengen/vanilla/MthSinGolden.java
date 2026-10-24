// MthSinGolden — proof that a locally computed Mth.SIN table is the JVM's.
//
// `Mth.sin`/`Mth.cos` are table lookups, not transcendentals: vanilla builds
// 65,536 floats once from `(float) Math.sin(i / 10430.378350470453)` and
// indexes them. Every carver tunnel, rotation and ellipsoid radius in the game
// goes through that table, so a port that calls the platform sine generates a
// different — smoother — world.
//
// The table is cheap to recompute and expensive to commit (256 KB), so the
// port computes it and this proves the result. `Math.sin` is only specified to
// within 1 ulp and HotSpot uses an intrinsic, so "Rust's f64::sin agrees" is a
// real claim rather than a formality; what makes it a safe bet is the narrowing
// to `float` afterwards, which throws away 29 bits.
//
// A SHA-256 over the whole table proves all 65,536 entries in 32 bytes. The
// spot samples alongside it are not redundant: a hash mismatch says only "some
// entry differs", and the samples give the first thing to look at. `Mth.sin`
// is private-free and public, so this needs no in-package access.
//
// Converted to JSON by `ci/mth-sin-golden.sh`.

import java.security.MessageDigest;

import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.Mth;

public final class MthSinGolden {

    private static final int SIN_QUANTIZATION = 65536;
    private static final double SIN_SCALE = 10430.378350470453;

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // Read back through `Mth.sin` rather than reflecting the private
        // array: what matters is the value the game actually gets, and the
        // index arithmetic is part of that. Feeding `i / SIN_SCALE` recovers
        // entry `i` exactly because the scaling is inverted before truncation.
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        StringBuilder samples = new StringBuilder();
        int sampleCount = 0;
        for (int i = 0; i < SIN_QUANTIZATION; i++) {
            float value = Mth.sin(i / SIN_SCALE);
            int bits = Float.floatToRawIntBits(value);
            // Little-endian, so the digest is reproducible from a Rust
            // `to_le_bytes()` without an endianness argument.
            digest.update(new byte[]{
                (byte) bits, (byte) (bits >> 8), (byte) (bits >> 16), (byte) (bits >> 24)
            });
            if (i % 4096 == 0) {
                samples.append(sampleCount++ == 0 ? "" : ", ")
                       .append('[').append(i).append(", \"").append(bits).append("\"]");
            }
        }

        StringBuilder hex = new StringBuilder();
        for (byte b : digest.digest()) {
            hex.append(String.format("%02x", b));
        }

        // A handful of angles as the callers actually spell them, so the
        // index arithmetic is covered too and not just the table contents.
        StringBuilder calls = new StringBuilder();
        int callCount = 0;
        double[] angles = {
            0.0, 1.0, -1.0, Math.PI, Math.PI / 2, -Math.PI / 2, 6.283185307179586,
            0.3, -0.37, 1000.0, -1000.0, 1.0e7, -1.0e7, 1.0e18, -1.0e18,
        };
        for (double angle : angles) {
            calls.append(callCount++ == 0 ? "" : ", ")
                 .append("{\"angle\": \"").append(Double.doubleToRawLongBits(angle))
                 .append("\", \"sin\": \"").append(Float.floatToRawIntBits(Mth.sin(angle)))
                 .append("\", \"cos\": \"").append(Float.floatToRawIntBits(Mth.cos(angle)))
                 .append("\"}");
        }

        // `OreFeature.place` calls **`Math.sin`/`Math.cos`**, not `Mth.sin` —
        // the platform transcendental, on a float angle widened to double,
        // with the result feeding straight into doubles and never narrowed.
        // So the 1-ulp slack in `Math.sin`'s specification is not absorbed by
        // a float rounding the way the table's is, and "Rust's f64::sin
        // agrees" is a claim that has to be settled rather than assumed.
        //
        // The angles are the ones the feature can actually produce:
        // `random.nextFloat() * (float) Math.PI`, so a float in [0, PI).
        StringBuilder platform = new StringBuilder();
        int platformCount = 0;
        for (int i = 0; i <= 4096; i++) {
            float angle = (float) (i / 4096.0) * (float) Math.PI;
            platform.append(platformCount++ == 0 ? "" : ", ")
                    .append("[\"").append(Float.floatToRawIntBits(angle))
                    .append("\", \"").append(Double.doubleToRawLongBits(Math.sin(angle)))
                    .append("\", \"").append(Double.doubleToRawLongBits(Math.cos(angle)))
                    .append("\"]");
        }

        System.out.println("{");
        System.out.println("  \"rng\": \"mth_sin\",");
        System.out.println("  \"quantization\": " + SIN_QUANTIZATION + ",");
        System.out.println("  \"tableSha256\": \"" + hex + "\",");
        System.out.println("  \"samples\": [" + samples + "],");
        System.out.println("  \"calls\": [" + calls + "],");
        System.out.println("  \"platform\": [" + platform + "]");
        System.out.println("}");
    }

    private MthSinGolden() {}
}
