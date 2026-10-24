// SkyGolden — vanilla's own answer for the two sky attributes this server
// reads, sampled out of the real `minecraft:timeline` registry.
//
// WHY THIS IS NOT A TRANSCRIPTION TEST
//
// `crate::sky` ports four pieces of machinery — `EasingType.CubicBezier`'s
// Newton-Raphson solve, `KeyframeTrackSampler`'s wrap segments,
// `FloatModifier` and `ValueSampler`'s layer fold. A unit test written from
// the same reading of the same bytecode proves only that the reading was
// copied twice. This asks the JVM instead.
//
// `ClockManager` is a functional interface — one method, `getTotalTicks` —
// so a sampler can be driven to an arbitrary tick without a world, a save
// directory or a tick loop. `AttributeTrackSampler.applyTimeBased` caches its
// sampled argument by tick id, so every sample here passes a distinct one;
// reusing an id would return the previous tick's value and the whole table
// would be one repeated number that still looked plausible.
//
// WHAT THIS DOES AND DOES NOT PIN
//
// It pins the inputs of `DaylightDetectorBlock.updateSignalStrength` — the
// sun angle in degrees and the sky light level — plus `Mth.cos` at exactly
// the angles that method will ask for. What it does not pin is the six lines
// of integer arithmetic between them, because `updateSignalStrength` is
// `private static` and reaching it needs a real `ServerLevel`. That half is
// covered by the corpus, and the split is stated here so nobody reads a green
// on this file as covering the block.
//
// Converted to JSON by `ci/sky-golden.sh`.

import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.Mth;
import net.minecraft.world.attribute.EnvironmentAttributes;
import net.minecraft.world.clock.ClockManager;
import net.minecraft.world.timeline.AttributeTrackSampler;
import net.minecraft.world.timeline.Timeline;
import net.minecraft.world.timeline.Timelines;

public final class SkyGolden {

    /// `EnvironmentAttributes.SKY_LIGHT_LEVEL`'s registered default, and the
    /// base `Level.updateSkyBrightness` subtracts the sampled value from.
    private static final float SKY_LIGHT_LEVEL_DEFAULT = 15.0f;
    /// `EnvironmentAttributes.SUN_ANGLE`'s registered default.
    private static final float SUN_ANGLE_DEFAULT = 0.0f;
    /// `updateSignalStrength` offset 23.
    private static final float DEG_TO_RAD = 0.017453292f;

    /// The sample grid. Dense enough near the two zero crossings of
    /// `cos(sunAngle)` that a wrong easing cannot hide between samples, and
    /// carrying both ends of every keyframe in `sky_light_level` — 133,
    /// 11867, 13670 and 22330 — plus one tick either side of each, because a
    /// segment boundary is where an off-by-one in `getSegmentAt` lives.
    private static int[] samples() {
        List<Integer> t = new ArrayList<>();
        for (int i = 0; i < 24000; i += 250) {
            t.add(i);
        }
        for (int edge : new int[] { 0, 132, 133, 134, 5999, 6000, 6001, 11866, 11867, 11868,
                13669, 13670, 13671, 22329, 22330, 22331, 23999 }) {
            t.add(edge);
        }
        // **The only two ticks of the day at which `Mth.cos` and a platform
        // cosine give a daylight detector a different `power`.** Swept over
        // all 24,000: at 2447 the table says 12 and `libm` says 13, at 10625
        // the table says 11 and `libm` says 10. Two ticks in 24,000 is
        // 0.008% of the day, so a sample grid that did not name them would
        // let the whole lookup-table port be replaced by `cos()` and stay
        // green — which is the difference between a golden and a decoration.
        t.add(2447);
        t.add(10625);
        // Both zero crossings of the detector's cosine, walked one tick at a
        // time: this is where `round(15 * cos)` changes answer.
        for (int i = 12550; i <= 12750; i++) {
            t.add(i);
        }
        for (int i = 1450; i <= 1650; i++) {
            t.add(i);
        }
        return t.stream().distinct().sorted().mapToInt(Integer::intValue).toArray();
    }

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        Timeline day = registries.lookupOrThrow(Registries.TIMELINE)
                .getOrThrow(Timelines.OVERWORLD_DAY).value();

        long[] now = { 0L };
        ClockManager clock = holder -> now[0];

        AttributeTrackSampler<Float, ?> sun =
                day.createTrackSampler(EnvironmentAttributes.SUN_ANGLE, clock);
        AttributeTrackSampler<Float, ?> sky =
                day.createTrackSampler(EnvironmentAttributes.SKY_LIGHT_LEVEL, clock);

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"what\": \"minecraft:day sampled through vanilla's own AttributeTrackSampler\",\n");
        out.append("  \"timeline\": \"minecraft:day\",\n");
        out.append("  \"period_ticks\": ").append(day.periodTicks().orElse(-1)).append(",\n");
        out.append("  \"sky_light_level_default\": ").append(SKY_LIGHT_LEVEL_DEFAULT).append(",\n");
        out.append("  \"sun_angle_default\": ").append(SUN_ANGLE_DEFAULT).append(",\n");
        out.append("  \"samples\": [\n");

        int[] ticks = samples();
        // Tick ids must be distinct per sampler call; `applyTimeBased` keys
        // its cache on them. `2 * i` and `2 * i + 1` keep them distinct across
        // the two samplers as well, which costs nothing and removes a way for
        // this file to be quietly wrong.
        for (int i = 0; i < ticks.length; i++) {
            int t = ticks[i];
            now[0] = t;
            float angle = sun.applyTimeBased(SUN_ANGLE_DEFAULT, 2 * i);
            float level = sky.applyTimeBased(SKY_LIGHT_LEVEL_DEFAULT, 2 * i + 1);
            int skyDarken = (int) (SKY_LIGHT_LEVEL_DEFAULT - level);
            // `Mth.cos` at exactly the argument `updateSignalStrength` builds:
            // the angle in radians after its 0.2 pull toward 0 or 2*pi.
            float a = angle * DEG_TO_RAD;
            float pull = a < (float) Math.PI ? 0.0f : 6.2831855f;
            float pulled = a + (pull - a) * 0.2f;
            out.append("    {\"ticks\": ").append(t)
                    .append(", \"sun_angle\": ").append(bits(angle))
                    .append(", \"sky_light_level\": ").append(bits(level))
                    .append(", \"sky_darken\": ").append(skyDarken)
                    .append(", \"pulled_radians\": ").append(bits(pulled))
                    .append(", \"mth_cos\": ").append(bits(Mth.cos(pulled)))
                    .append("}");
            out.append(i + 1 < ticks.length ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    /// Floats are emitted as their raw bits, not as decimal text.
    ///
    /// A `sun_angle` of `282.3714` printed and re-parsed is a different float
    /// from the one the JVM held, and the whole point of this table is that
    /// the last bit decides whether `round(15 * cos)` is 7 or 8 at the
    /// crossings. The Rust side compares bit patterns.
    private static String bits(float f) {
        return Integer.toString(Float.floatToRawIntBits(f));
    }
}
