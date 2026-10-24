// AttributesGolden — the attribute registry and every entity type's base map,
// read off the pinned jar rather than transcribed.
//
// WHY THIS EXISTS AND WHY IT IS NOT THREE CONSTANTS
//
// `Entity.getDefaultGravity()` is a **method with overrides**, and the split
// between them is the whole reason this file is here:
//
//   Entity.getDefaultGravity            -> dconst_0
//   ItemEntity.getDefaultGravity        -> ldc2_w 0.04d
//   FallingBlockEntity.getDefaultGravity-> ldc2_w 0.04d
//   LivingEntity.getDefaultGravity      -> getAttributeValue(Attributes.GRAVITY)
//
// So two of the three entity kinds this server simulates read a literal and
// the third reads an attribute. Hardcoding `0.08` for the third would be a
// number transcribed from bytecode into Rust with nothing checking it, and it
// would be wrong the moment anything carries a modifier — a slow-falling
// potion, an attribute command, a `minecraft:gravity` modifier on a chestplate.
// The value is *also* clamped: `RangedAttribute.sanitizeValue` is
// `Mth.clamp(v, minValue, maxValue)` with NaN mapped to `minValue`, and the
// clamp is a property of the attribute rather than of the caller.
//
// This dumper therefore emits the whole path's *data* — every attribute with
// its default and its range, and every entity type's `AttributeSupplier` base
// map — and `crates/rustmc-server/src/attributes.rs` implements only the
// *arithmetic*, which is the part that is a program rather than a number.
//
// WHAT IS DELIBERATELY NOT HERE
//
// Modifiers. `AttributeInstance` holds them in an
// `EnumMap<Operation, Map<Identifier, AttributeModifier>>` populated at
// runtime by equipment, effects and commands, none of which this server has.
// The *arithmetic* over them is ported (see `attributes.rs`) so that the day
// one appears it is applied in vanilla's order; the modifier set itself is
// empty and is not something a static dump could carry.
//
// OUTPUT
//
// Every double is emitted twice: once readable, once as the raw IEEE-754 bit
// pattern under a `Bits` suffix. **The reader reads the bits.** `0.08` is not
// the double vanilla holds — `Attributes.GRAVITY`'s default is the `double`
// literal `0.08d`, and a JSON round trip through a shortest-repr printer is
// only exact if every stage is correctly rounded. The bits make the question
// moot, which is the same argument `collision_shapes.json` uses for its
// coordinate lists.

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
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.ai.attributes.Attribute;
import net.minecraft.world.entity.ai.attributes.AttributeSupplier;
import net.minecraft.world.entity.ai.attributes.DefaultAttributes;
import net.minecraft.world.entity.ai.attributes.RangedAttribute;

public final class AttributesGolden {
    private AttributesGolden() {}

    private static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    /** A double, readable and exact, as two JSON fields. */
    private static String dbl(String name, double v) {
        return q(name) + ": " + v + ", " + q(name + "Bits") + ": " + Double.doubleToRawLongBits(v);
    }

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // ---- every attribute, sorted by id so the file is stable ----
        Map<String, Attribute> attrs = new TreeMap<>();
        for (Identifier id : BuiltInRegistries.ATTRIBUTE.keySet()) {
            attrs.put(id.toString(), BuiltInRegistries.ATTRIBUTE.getValue(id));
        }

        List<String> attrRows = new ArrayList<>();
        for (Map.Entry<String, Attribute> e : attrs.entrySet()) {
            Attribute a = e.getValue();
            StringBuilder r = new StringBuilder();
            r.append("    {").append(q("id")).append(": ").append(q(e.getKey()));
            r.append(", ").append(dbl("default", a.getDefaultValue()));
            // `Attribute.sanitizeValue` is the identity; `RangedAttribute`
            // overrides it with the clamp. The distinction is carried rather
            // than assumed, because a reader that clamped everything to
            // [default, default] for a plain Attribute would be silently
            // wrong and nothing in this server would notice.
            if (a instanceof RangedAttribute ra) {
                r.append(", ").append(q("ranged")).append(": true");
                r.append(", ").append(dbl("min", ra.getMinValue()));
                r.append(", ").append(dbl("max", ra.getMaxValue()));
            } else {
                r.append(", ").append(q("ranged")).append(": false");
            }
            r.append(", ").append(q("syncable")).append(": ").append(a.isClientSyncable());
            r.append(", ").append(q("descriptionId")).append(": ").append(q(a.getDescriptionId()));
            r.append("}");
            attrRows.add(r.toString());
        }

        // ---- every entity type that has an AttributeSupplier ----
        //
        // `DefaultAttributes.SUPPLIERS` is keyed by `EntityType<? extends
        // LivingEntity>`, so a type with no supplier is a type with no
        // attributes at all — an item, a falling block, an arrow. That is a
        // fact worth carrying rather than inferring: it is exactly the set
        // for which `getAttributeValue` is not reachable, and therefore the
        // set whose `getDefaultGravity` cannot be an attribute read.
        List<String> supplierRows = new ArrayList<>();
        int typesWithSupplier = 0;
        int typesWithout = 0;
        Map<String, EntityType<?>> types = new TreeMap<>();
        for (Identifier id : BuiltInRegistries.ENTITY_TYPE.keySet()) {
            types.put(id.toString(), BuiltInRegistries.ENTITY_TYPE.getValue(id));
        }
        for (Map.Entry<String, EntityType<?>> e : types.entrySet()) {
            EntityType<?> t = e.getValue();
            if (!DefaultAttributes.hasSupplier(t)) {
                typesWithout++;
                continue;
            }
            typesWithSupplier++;
            @SuppressWarnings("unchecked")
            AttributeSupplier sup =
                    DefaultAttributes.getSupplier((EntityType<? extends LivingEntity>) t);
            List<String> base = new ArrayList<>();
            for (Map.Entry<String, Attribute> ae : attrs.entrySet()) {
                Holder<Attribute> h = BuiltInRegistries.ATTRIBUTE.wrapAsHolder(ae.getValue());
                if (!sup.hasAttribute(h)) {
                    continue;
                }
                base.add("        {" + q("id") + ": " + q(ae.getKey()) + ", "
                        + dbl("base", sup.getBaseValue(h)) + "}");
            }
            supplierRows.add("    {" + q("type") + ": " + q(e.getKey()) + ", " + q("base")
                    + ": [\n" + String.join(",\n", base) + "\n    ]}");
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("attributes")).append(": ").append(attrs.size()).append(",\n");
        out.append("  ").append(q("typesWithSupplier")).append(": ").append(typesWithSupplier)
                .append(",\n");
        out.append("  ").append(q("typesWithoutSupplier")).append(": ").append(typesWithout)
                .append(",\n");
        out.append("  ").append(q("table")).append(": [\n");
        out.append(String.join(",\n", attrRows));
        out.append("\n  ],\n");
        out.append("  ").append(q("suppliers")).append(": [\n");
        out.append(String.join(",\n", supplierRows));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }
}
