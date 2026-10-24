// PotionsGolden — the whole `minecraft:potion` registry, read off the pinned
// jar rather than transcribed.
//
// WHY THIS IS ENTIRELY A TABLE, WITH NO PROGRAM BESIDE IT
//
// `Potion` has one field that matters — `List<MobEffectInstance> effects` —
// and three methods, none of which branches: `getEffects`, `name` and
// `hasInstantEffects`. Every potion in the game is a constructor call in
// `Potions.<clinit>` with a duration and an amplifier written into it, and
// `PotionContents` is the component that carries the list onto an item stack.
//
// So unlike `MobEffect`, there is nothing here to port. Forty-six rows of
// (effect, duration, amplifier) triples are exactly the kind of thing
// `docs/AGENT-BRIEF.md` means by "registry data is a table": a hand-transcribed
// `long_strength` at 4800 ticks instead of 4200 would be invisible to every
// test this project has, and would stay invisible until somebody timed a
// potion.
//
// WHAT THE TRIPLES ARE FOR
//
// `crates/rustmc-server/src/effects.rs` ports `MobEffectInstance`, which is the
// thing these triples construct. A drink applies each of a potion's instances
// through `MobEffectInstance.update` against whatever the player already holds,
// so the durations here are the input to the hidden-effect chain that module
// transcribes — and a corpus scenario that drinks a potion and watches the
// wire is reading this table's numbers back out.
//
// WHAT IS DELIBERATELY NOT HERE
//
// The brewing graph. `PotionBrewing` maps (input potion, ingredient) to an
// output potion and is a different registry-shaped thing with its own
// consumer; nothing on this server brews, and a table nothing reads is a
// table that goes stale unnoticed.
//
// The item that carries a potion, too: `minecraft:potion`,
// `minecraft:splash_potion`, `minecraft:lingering_potion` and
// `minecraft:tipped_arrow` all take a `PotionContents` component, and which
// item a stack is belongs to `data/vanilla/items.json`.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.item.alchemy.Potion;

public final class PotionsGolden {
    private PotionsGolden() {}

    private static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        Map<String, Potion> potions = new TreeMap<>();
        for (Identifier id : BuiltInRegistries.POTION.keySet()) {
            potions.put(id.toString(), BuiltInRegistries.POTION.getValue(id));
        }

        int withInstant = 0;
        int empty = 0;
        int instances = 0;
        List<String> rows = new ArrayList<>();

        for (Map.Entry<String, Potion> e : potions.entrySet()) {
            Potion p = e.getValue();
            StringBuilder r = new StringBuilder();
            r.append("    {").append(q("id")).append(": ").append(q(e.getKey()));
            // The wire value: `Potion.STREAM_CODEC` is a holder registry codec,
            // so this number is what a `PotionContents` component serialises.
            r.append(", ").append(q("registryId")).append(": ")
                    .append(BuiltInRegistries.POTION.getId(p));
            // `name()` is the translation stem, and it is NOT the registry
            // path: `long_night_vision` is named `night_vision`, which is how
            // the three strengths of one potion share a display name.
            r.append(", ").append(q("name")).append(": ").append(q(p.name()));
            r.append(", ").append(q("hasInstantEffects")).append(": ")
                    .append(p.hasInstantEffects());

            if (p.hasInstantEffects()) {
                withInstant++;
            }
            if (p.getEffects().isEmpty()) {
                empty++;
            }

            List<String> eff = new ArrayList<>();
            for (MobEffectInstance mi : p.getEffects()) {
                instances++;
                eff.add("        {" + q("effect") + ": "
                        + q(mi.getEffect().unwrapKey().orElseThrow().identifier().toString())
                        + ", " + q("duration") + ": " + mi.getDuration()
                        + ", " + q("amplifier") + ": " + mi.getAmplifier()
                        + ", " + q("ambient") + ": " + mi.isAmbient()
                        + ", " + q("visible") + ": " + mi.isVisible()
                        + ", " + q("showIcon") + ": " + mi.showIcon() + "}");
            }
            r.append(", ").append(q("effects")).append(": [\n")
                    .append(String.join(",\n", eff)).append("\n    ]}");
            rows.add(r.toString());
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("potions")).append(": ").append(potions.size()).append(",\n");
        out.append("  ").append(q("withInstantEffects")).append(": ").append(withInstant)
                .append(",\n");
        out.append("  ").append(q("withNoEffects")).append(": ").append(empty).append(",\n");
        out.append("  ").append(q("effectInstances")).append(": ").append(instances).append(",\n");
        out.append("  ").append(q("table")).append(": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }
}
