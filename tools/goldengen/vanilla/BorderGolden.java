package net.minecraft.world.level.levelgen;

import io.netty.buffer.Unpooled;
import java.io.PrintStream;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.protocol.game.ClientboundInitializeBorderPacket;
import net.minecraft.network.protocol.game.ClientboundSetBorderCenterPacket;
import net.minecraft.network.protocol.game.ClientboundSetBorderLerpSizePacket;
import net.minecraft.network.protocol.game.ClientboundSetBorderSizePacket;
import net.minecraft.network.protocol.game.ClientboundSetBorderWarningDelayPacket;
import net.minecraft.network.protocol.game.ClientboundSetBorderWarningDistancePacket;
import net.minecraft.world.level.border.WorldBorder;

/**
 * The world border: vanilla's own numbers for the lerp, the box and the six
 * clientbound packets, captured at a fixed script of operations.
 *
 * <p>Two things are captured together on purpose. The packets are built from a
 * {@link WorldBorder} — every one of the six constructors takes one and reads
 * fields off it — so a capture of the bytes is also a capture of the border
 * state that produced them. A transcription that gets the lerp right and the
 * wire wrong, or the wire right and the lerp wrong, moves these bytes.
 *
 * <p>What would make this vacuous: a script that never leaves the default
 * state. The default border is 5.9999968E7 wide at the origin, which is the
 * one configuration where nearly every field is a constant and every mistake
 * cancels. So the script below moves the centre off the origin and off the
 * axes, sets an odd size, runs a shrink and a grow, and ticks *through* the
 * end of a lerp so the extent has to swap itself back to static.
 *
 * <p>Run by {@code ci/border-golden.sh}. Writes to
 * {@code crates/rustmc-server/golden/world_border.json} — a fixture compared
 * against, not an input the server reads.
 */
public final class BorderGolden {
    private BorderGolden() {}

    /** A probe point that is inside the small borders and outside the big one. */
    private static final double PROBE_X = 137.5;
    private static final double PROBE_Z = -42.25;

    public static void main(String[] args) {
        PrintStream out = System.out;
        List<String> states = new ArrayList<>();

        WorldBorder b = new WorldBorder();
        states.add(state("fresh", b));

        b.setCenter(137.0, -42.0);
        states.add(state("center-137--42", b));

        b.setSize(64.0);
        states.add(state("size-64", b));

        b.setWarningBlocks(9);
        b.setWarningTime(37);
        b.setDamagePerBlock(0.35);
        b.setSafeZone(1.25);
        states.add(state("warn-9-37-dmg-0.35-safe-1.25", b));

        // A shrink over 20 ticks, sampled at the start, in the middle, on the
        // last moving tick and one tick past the end — where `update()` must
        // hand back a StaticBorderExtent and the size must be exactly `to`.
        b.lerpSizeBetween(64.0, 12.0, 20L, 0L);
        states.add(state("shrink-t0", b));
        for (int i = 0; i < 10; i++) {
            b.tick();
        }
        states.add(state("shrink-t10", b));
        for (int i = 0; i < 9; i++) {
            b.tick();
        }
        states.add(state("shrink-t19", b));
        b.tick();
        states.add(state("shrink-t20", b));
        b.tick();
        states.add(state("shrink-t21", b));

        // A grow whose duration does not divide its distance, so a truncating
        // step would land somewhere a lerp does not.
        b.lerpSizeBetween(12.0, 100.0, 7L, 0L);
        states.add(state("grow-t0", b));
        b.tick();
        b.tick();
        b.tick();
        states.add(state("grow-t3", b));

        // A zero-length "lerp": vanilla's `lerpSizeBetween` compares `from` to
        // `to` and installs a *static* extent when they are equal, so the
        // status here is STATIONARY and not GROWING.
        b.lerpSizeBetween(50.0, 50.0, 40L, 0L);
        states.add(state("degenerate-equal", b));

        // The absolute-max clamp: a border wider than 2 * absoluteMaxSize has
        // its box clamped and its size left alone, so `size` and
        // `maxX - minX` stop agreeing. That divergence is the assertion.
        b.setAbsoluteMaxSize(64);
        b.setSize(1000.0);
        states.add(state("clamped-absmax-64", b));

        out.println("{");
        out.println("  \"source\": \"net.minecraft.world.level.border.WorldBorder, 26.2 bytecode\",");
        out.printf("  \"maxSizeBits\": %d,%n", Double.doubleToRawLongBits(WorldBorder.MAX_SIZE));
        out.printf(
                "  \"maxCenterCoordinateBits\": %d,%n",
                Double.doubleToRawLongBits(WorldBorder.MAX_CENTER_COORDINATE));
        out.printf("  \"probeXBits\": %d,%n", Double.doubleToRawLongBits(PROBE_X));
        out.printf("  \"probeZBits\": %d,%n", Double.doubleToRawLongBits(PROBE_Z));
        // `WorldBorder.clampToBounds(double, double, double)`, which is what
        // `NetherPortalBlock.getPortalDestination` puts the scaled entry
        // position through. Two things a reimplementation gets wrong: the
        // upper clamp subtracts `(double)(float)1.0E-5` — a FLOAT literal
        // widened, so 9.999999747378752E-6 and not 1.0E-5 — and **y is not
        // clamped at all**, only floored. Captured against a small border so
        // the clamp actually bites.
        WorldBorder small = new WorldBorder();
        small.setCenter(0.0, 0.0);
        small.setSize(32.0);
        out.println("  \"clampSize\": 32.0,");
        out.println("  \"clamps\": [");
        double[][] probes = {
            {0.0, 64.0, 0.0},
            {15.5, 64.0, -15.5},
            {16.0, 64.0, 16.0},
            {1000.0, 320.0, -1000.0},
            {-1000.0, -64.0, 1000.0},
            {15.999999, 70.5, 15.999999},
            {-16.0, 70.5, -16.0},
        };
        for (int i = 0; i < probes.length; i++) {
            net.minecraft.core.BlockPos c = small.clampToBounds(probes[i][0], probes[i][1], probes[i][2]);
            out.printf(
                    "    {\"in\": [%s, %s, %s], \"out\": [%d, %d, %d]}%s%n",
                    d(probes[i][0]),
                    d(probes[i][1]),
                    d(probes[i][2]),
                    c.getX(),
                    c.getY(),
                    c.getZ(),
                    i + 1 < probes.length ? "," : "");
        }
        out.println("  ],");
        out.println("  \"states\": [");
        for (int i = 0; i < states.size(); i++) {
            out.print("    " + states.get(i));
            out.println(i + 1 < states.size() ? "," : "");
        }
        out.println("  ]");
        out.println("}");
    }

    /**
     * One border state: every scalar the server has to reproduce, plus the six
     * packets vanilla would send from it.
     */
    private static String state(String name, WorldBorder b) {
        StringBuilder s = new StringBuilder();
        s.append("{\"name\": \"").append(name).append("\"");
        dbl(s, "centerX", b.getCenterX());
        dbl(s, "centerZ", b.getCenterZ());
        dbl(s, "size", b.getSize());
        dbl(s, "lerpTarget", b.getLerpTarget());
        s.append(", \"lerpTime\": ").append(b.getLerpTime());
        dbl(s, "lerpSpeed", b.getLerpSpeed());
        s.append(", \"status\": \"").append(b.getStatus().name()).append("\"");
        dbl(s, "minX", b.getMinX());
        dbl(s, "minZ", b.getMinZ());
        dbl(s, "maxX", b.getMaxX());
        dbl(s, "maxZ", b.getMaxZ());
        s.append(", \"warningBlocks\": ").append(b.getWarningBlocks());
        s.append(", \"warningTime\": ").append(b.getWarningTime());
        dbl(s, "damagePerBlock", b.getDamagePerBlock());
        dbl(s, "safeZone", b.getSafeZone());
        s.append(", \"absoluteMaxSize\": ").append(b.getAbsoluteMaxSize());
        dbl(s, "distanceToProbe", b.getDistanceToBorder(PROBE_X, PROBE_Z));
        s.append(", \"probeWithinBounds\": ").append(b.isWithinBounds(PROBE_X, PROBE_Z));
        s.append(", \"packets\": {");
        s.append("\"initialize_border\": \"")
                .append(hex(ClientboundInitializeBorderPacket.STREAM_CODEC, new ClientboundInitializeBorderPacket(b)))
                .append("\"");
        s.append(", \"set_border_center\": \"")
                .append(hex(ClientboundSetBorderCenterPacket.STREAM_CODEC, new ClientboundSetBorderCenterPacket(b)))
                .append("\"");
        s.append(", \"set_border_lerp_size\": \"")
                .append(hex(ClientboundSetBorderLerpSizePacket.STREAM_CODEC, new ClientboundSetBorderLerpSizePacket(b)))
                .append("\"");
        s.append(", \"set_border_size\": \"")
                .append(hex(ClientboundSetBorderSizePacket.STREAM_CODEC, new ClientboundSetBorderSizePacket(b)))
                .append("\"");
        s.append(", \"set_border_warning_delay\": \"")
                .append(hex(
                        ClientboundSetBorderWarningDelayPacket.STREAM_CODEC,
                        new ClientboundSetBorderWarningDelayPacket(b)))
                .append("\"");
        s.append(", \"set_border_warning_distance\": \"")
                .append(hex(
                        ClientboundSetBorderWarningDistancePacket.STREAM_CODEC,
                        new ClientboundSetBorderWarningDistancePacket(b)))
                .append("\"");
        s.append("}}");
        return s.toString();
    }

    private static <T> String hex(net.minecraft.network.codec.StreamCodec<? super FriendlyByteBuf, T> codec, T pkt) {
        FriendlyByteBuf buf = new FriendlyByteBuf(Unpooled.buffer());
        codec.encode(buf, pkt);
        byte[] b = new byte[buf.readableBytes()];
        buf.readBytes(b);
        StringBuilder sb = new StringBuilder(b.length * 2);
        for (byte x : b) {
            sb.append(String.format("%02x", x));
        }
        return sb.toString();
    }

    /**
     * A double written so it reads back bit-identical. `Double.toString` is the
     * shortest round-tripping form in Java and Rust's `f64::from_str` accepts
     * it, but `Infinity` and `NaN` are not JSON, so those are refused rather
     * than emitted: a border that produces one is a finding, not a row.
     */
    /**
     * A double field, written twice: once as a decimal for a reader and once
     * as its IEEE-754 bit pattern for a comparator.
     *
     * <p>The bit pattern is not belt-and-braces. `serde_json` without its
     * `float_roundtrip` feature — which is the default, and what this
     * workspace builds — parses a 17-significant-digit decimal one ULP away
     * from the double that produced it. Measured here: this capture's
     * `shrink-t19` size is `14.600000000000001` (bits ...556) and
     * `as_f64()` hands back `14.6` (bits ...555). A golden compared through
     * the decimal is therefore wrong in the last bit *and cannot be made
     * right by writing more digits*, so the comparison is moved off the
     * decimal entirely.
     */
    private static void dbl(StringBuilder s, String name, double v) {
        s.append(", \"").append(name).append("\": ").append(d(v));
        s.append(", \"").append(name).append("Bits\": ").append(Double.doubleToRawLongBits(v));
    }

    private static String d(double v) {
        if (Double.isNaN(v) || Double.isInfinite(v)) {
            throw new IllegalStateException("non-finite border value: " + v);
        }
        return Double.toString(v);
    }
}
