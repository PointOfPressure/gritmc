// MobEffectsGolden — the whole `minecraft:mob_effect` registry, read off the
// pinned jar rather than transcribed.
//
// WHY THIS IS A TABLE AND `effects.rs` IS A PROGRAM
//
// Two different things live in `MobEffect` and they need opposite treatment.
//
//   * **Per-effect data** — the category, the colour, the blend durations, the
//     attribute modifiers each effect grants and the amount of each at a given
//     amplifier. Forty rows of constants set in `MobEffects.<clinit>` by a
//     builder chain. Transcribing forty rows by hand is forty chances to
//     mistype a double, and nothing downstream would notice: a wrong
//     `movement_speed` amount on Speed II is invisible until somebody measures
//     a sprint.
//
//   * **The tick rule** — `shouldApplyEffectTickThisTick(duration, amplifier)`,
//     which is a *program* with five distinct shapes across ten subclasses. It
//     is ported in `crates/rustmc-server/src/effects.rs`.
//
// This dumper carries both, and the second one is the point: it **probes** the
// rule over a grid rather than reading the classes, so the Rust port is checked
// against vanilla's own answers instead of against a second reading of the
// same bytecode by the same pair of eyes. `effects::tests::
// the_transcribed_rules_agree_with_the_probed_table` is that check.
//
// WHAT THE PROBE COVERS AND WHY THOSE BOUNDS
//
// Durations 0..=120 and amplifiers 0..=6. The three periodic rules have
// pre-shift bases of 25, 40 and 50, so 120 covers between two and four full
// periods of each; and `50 >> 6` is 0, so amplifier 6 reaches the
// shift-to-zero arm of all three. An amplifier of 32 is probed separately
// because Java's `int` shift masks its count to five bits and `25 >> 32` is
// therefore 25 — a case a 0..=6 sweep cannot see and a Rust `>>` would panic
// on in debug.
//
// WHAT IS DELIBERATELY NOT HERE
//
// `applyEffectTick`'s *effect*. It takes a `ServerLevel` and a `LivingEntity`
// and heals, hurts or exhausts them; there is no cheap oracle for that, so it
// is hand-ported per class with its own tests. This is the same split
// `data/vanilla/use_without_item.json` documents for `Block.tick`.
//
// OUTPUT
//
// Doubles are emitted twice, readable and as raw IEEE-754 bits, and the bits
// are what a reader should take — same argument as `AttributesGolden`.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.effect.MobEffect;
import net.minecraft.world.entity.ai.attributes.Attribute;
import net.minecraft.world.entity.ai.attributes.AttributeModifier;

public final class MobEffectsGolden {
    private MobEffectsGolden() {}

    /** Durations probed, inclusive. */
    private static final int MAX_DURATION = 120;
    /** Amplifiers probed, inclusive, plus the masked-shift case below. */
    private static final int MAX_AMPLIFIER = 6;
    /** The one amplifier that exercises Java's five-bit shift mask. */
    private static final int MASKED_SHIFT_AMPLIFIER = 32;

    private static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private static String dbl(String name, double v) {
        return q(name) + ": " + v + ", " + q(name + "Bits") + ": " + Double.doubleToRawLongBits(v);
    }

    /** The durations in 0..=MAX_DURATION at which the rule fires. */
    private static String firesAt(MobEffect effect, int amplifier) {
        List<String> hits = new ArrayList<>();
        for (int d = 0; d <= MAX_DURATION; d++) {
            if (effect.shouldApplyEffectTickThisTick(d, amplifier)) {
                hits.add(Integer.toString(d));
            }
        }
        return "[" + String.join(",", hits) + "]";
    }

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        Map<String, MobEffect> effects = new TreeMap<>();
        for (Identifier id : BuiltInRegistries.MOB_EFFECT.keySet()) {
            effects.put(id.toString(), BuiltInRegistries.MOB_EFFECT.getValue(id));
        }

        int instantaneous = 0;
        int beneficial = 0;
        int withModifiers = 0;
        int everTicks = 0;
        List<String> rows = new ArrayList<>();

        for (Map.Entry<String, MobEffect> e : effects.entrySet()) {
            MobEffect m = e.getValue();
            StringBuilder r = new StringBuilder();
            r.append("    {").append(q("id")).append(": ").append(q(e.getKey()));
            // The registry id is the number on the wire in
            // `ClientboundUpdateMobEffectPacket`, so it is data a port needs
            // and not merely an index into this file.
            r.append(", ").append(q("registryId")).append(": ")
                    .append(BuiltInRegistries.MOB_EFFECT.getId(m));
            r.append(", ").append(q("category")).append(": ").append(q(m.getCategory().name()));
            r.append(", ").append(q("color")).append(": ").append(m.getColor());
            r.append(", ").append(q("descriptionId")).append(": ").append(q(m.getDescriptionId()));
            r.append(", ").append(q("instantaneous")).append(": ").append(m.isInstantaneous());
            r.append(", ").append(q("beneficial")).append(": ").append(m.isBeneficial());
            r.append(", ").append(q("blendInTicks")).append(": ")
                    .append(m.getBlendInDurationTicks());
            r.append(", ").append(q("blendOutTicks")).append(": ")
                    .append(m.getBlendOutDurationTicks());
            r.append(", ").append(q("blendOutAdvanceTicks")).append(": ")
                    .append(m.getBlendOutAdvanceTicks());
            // The implementing class. It is the thing `effects.rs`'s TickRule
            // enum has one variant per, so a new subclass in a later version
            // shows up here as an unrecognised name rather than as silence.
            r.append(", ").append(q("class")).append(": ")
                    .append(q(m.getClass().getSimpleName()));

            if (m.isInstantaneous()) {
                instantaneous++;
            }
            if (m.isBeneficial()) {
                beneficial++;
            }

            // ---- the tick-rule probe ----
            List<String> probe = new ArrayList<>();
            boolean ticks = false;
            for (int a = 0; a <= MAX_AMPLIFIER; a++) {
                String hits = firesAt(m, a);
                if (!hits.equals("[]")) {
                    ticks = true;
                }
                probe.add("        {" + q("amplifier") + ": " + a + ", " + q("firesAt") + ": "
                        + hits + "}");
            }
            probe.add("        {" + q("amplifier") + ": " + MASKED_SHIFT_AMPLIFIER + ", "
                    + q("firesAt") + ": " + firesAt(m, MASKED_SHIFT_AMPLIFIER) + "}");
            if (ticks) {
                everTicks++;
            }
            r.append(", ").append(q("tick")).append(": [\n").append(String.join(",\n", probe))
                    .append("\n    ]");

            // ---- the attribute modifiers, at two amplifiers ----
            //
            // `createModifiers` is the only public way in: the map itself is
            // private and the AttributeTemplate's amount is a function of the
            // amplifier, so a single row could not carry it. Two amplifiers
            // are enough to pin both the constant and the per-level term of
            // every template vanilla ships, and a reader that needs a third
            // has the same call available.
            List<String> mods = new ArrayList<>();
            for (int a = 0; a <= 1; a++) {
                List<String> at = new ArrayList<>();
                final int amp = a;
                m.createModifiers(amp, (Holder<Attribute> attr, AttributeModifier mod) -> {
                    at.add("            {" + q("attribute") + ": "
                            + q(attr.unwrapKey().orElseThrow().identifier().toString()) + ", "
                            + q("modifierId") + ": " + q(mod.id().toString()) + ", "
                            + dbl("amount", mod.amount()) + ", " + q("operation") + ": "
                            + q(mod.operation().name()) + "}");
                });
                if (!at.isEmpty() && amp == 0) {
                    withModifiers++;
                }
                mods.add("        {" + q("amplifier") + ": " + amp + ", " + q("modifiers")
                        + ": [\n" + String.join(",\n", at) + "\n        ]}");
            }
            r.append(", ").append(q("attributeModifiers")).append(": [\n")
                    .append(String.join(",\n", mods)).append("\n    ]");
            r.append("}");
            rows.add(r.toString());
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("effects")).append(": ").append(effects.size()).append(",\n");
        out.append("  ").append(q("instantaneous")).append(": ").append(instantaneous)
                .append(",\n");
        out.append("  ").append(q("beneficial")).append(": ").append(beneficial).append(",\n");
        out.append("  ").append(q("withAttributeModifiers")).append(": ").append(withModifiers)
                .append(",\n");
        out.append("  ").append(q("everTicks")).append(": ").append(everTicks).append(",\n");
        out.append("  ").append(q("maxProbedDuration")).append(": ").append(MAX_DURATION)
                .append(",\n");
        out.append("  ").append(q("maxProbedAmplifier")).append(": ").append(MAX_AMPLIFIER)
                .append(",\n");
        out.append("  ").append(q("maskedShiftAmplifier")).append(": ")
                .append(MASKED_SHIFT_AMPLIFIER).append(",\n");
        out.append("  ").append(q("table")).append(": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }
}
