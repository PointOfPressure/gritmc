// DamageTypesGolden — the `minecraft:damage_type` registry and every
// `DamageTypeTags` membership, read off the pinned jar rather than transcribed.
//
// WHY THIS ONE READS THE JAR'S OWN DATA FILES
//
// Damage types are a **datapack** registry in 26.2, not a `BuiltInRegistries`
// one: `data/minecraft/damage_type/<id>.json` and
// `data/minecraft/tags/damage_type/<tag>.json` are shipped inside the server
// jar and loaded by the datapack machinery at world load. So there is no
// static registry to walk the way `AttributesGolden` walks
// `BuiltInRegistries.ATTRIBUTE`, and standing a whole `ReloadableServerResources`
// up to reach one table would be a boot for a directory listing.
//
// This dumper therefore opens the jar as a zip and reads the two directories.
// That is a shortcut in *mechanism* and not in fidelity — those files are the
// registry, byte for byte, and they are the same bytes the server parses.
//
// WHAT MAKES IT ABLE TO FAIL RATHER THAN QUIETLY UNDER-REPORT
//
// A directory listing that misses files reports a smaller registry and nothing
// notices. So the set read out of the zip is cross-checked against
// `net.minecraft.world.damagesource.DamageTypes`, whose `public static final
// ResourceKey<DamageType>` fields are the canonical list the *code* refers to:
// every key declared there must have been read, and the dumper exits non-zero
// if one was not. The two sources are independent — one is a field list in a
// compiled interface, the other is a set of resource paths — so they cannot
// both be wrong in the same direction by accident.
//
// WHAT THE TAGS ARE FOR
//
// `crates/rustmc-server/src/health.rs` and `crate::hurt` between them read ten
// `DamageTypeTags` memberships — BYPASSES_ARMOR, BYPASSES_EFFECTS,
// BYPASSES_RESISTANCE, BYPASSES_ENCHANTMENTS, BYPASSES_COOLDOWN, IS_FIRE,
// IS_FREEZING, DAMAGES_HELMET, NO_IMPACT, NO_KNOCKBACK — as the `DamageTags`
// struct. This table is where that struct gets filled from.
//
// A tag file's `values` may name **another tag**, with a `#` prefix, and may
// name an optional entry as an object with `"required": false`. Both forms are
// resolved here — transitively for the first — because a reader that treated
// `#minecraft:is_fire` as an id would silently produce an empty tag.
//
// WHAT IS DELIBERATELY NOT HERE
//
// `DamageSource` itself. It is a runtime object pairing a type holder with up
// to two entities and a position, built per hit by `DamageSources`; there is
// nothing static about it to dump.

package net.minecraft.world.level.levelgen;

import java.io.InputStream;
import java.io.InputStreamReader;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Enumeration;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import net.minecraft.SharedConstants;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.damagesource.DamageType;
import net.minecraft.tags.DamageTypeTags;
import net.minecraft.tags.TagKey;
import net.minecraft.world.damagesource.DamageTypes;

public final class DamageTypesGolden {
    private DamageTypesGolden() {}

    private static final String TYPE_DIR = "data/minecraft/damage_type/";
    private static final String TAG_DIR = "data/minecraft/tags/damage_type/";

    private static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private static JsonObject read(ZipFile zip, ZipEntry entry) throws Exception {
        try (InputStream in = zip.getInputStream(entry);
                InputStreamReader r = new InputStreamReader(in, StandardCharsets.UTF_8)) {
            return JsonParser.parseReader(r).getAsJsonObject();
        }
    }

    /** Resolve one tag's `values`, following `#other_tag` references. */
    private static Set<String> resolve(String tag, Map<String, JsonObject> tags,
            Set<String> seen) {
        Set<String> out = new TreeSet<>();
        if (!seen.add(tag)) {
            return out; // a cycle; vanilla's loader would reject it, we stop
        }
        JsonObject o = tags.get(tag);
        if (o == null || !o.has("values")) {
            return out;
        }
        for (JsonElement v : o.getAsJsonArray("values")) {
            String name;
            if (v.isJsonObject()) {
                // {"id": "...", "required": false}
                name = v.getAsJsonObject().get("id").getAsString();
            } else {
                name = v.getAsString();
            }
            if (name.startsWith("#")) {
                out.addAll(resolve(name.substring(1), tags, seen));
            } else {
                out.add(name);
            }
        }
        return out;
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        String jarPath = System.getProperty("rustmc.jar");
        if (jarPath == null) {
            System.err.println("DamageTypesGolden needs -Drustmc.jar=<server jar>");
            System.exit(2);
        }

        Map<String, JsonObject> types = new TreeMap<>();
        Map<String, JsonObject> tagFiles = new TreeMap<>();
        try (ZipFile zip = new ZipFile(jarPath)) {
            Enumeration<? extends ZipEntry> entries = zip.entries();
            while (entries.hasMoreElements()) {
                ZipEntry e = entries.nextElement();
                String n = e.getName();
                if (n.startsWith(TYPE_DIR) && n.endsWith(".json")) {
                    types.put("minecraft:"
                            + n.substring(TYPE_DIR.length(), n.length() - 5), read(zip, e));
                } else if (n.startsWith(TAG_DIR) && n.endsWith(".json")) {
                    tagFiles.put("minecraft:"
                            + n.substring(TAG_DIR.length(), n.length() - 5), read(zip, e));
                }
            }
        }

        // ---- the independent cross-check ----
        //
        // `DamageTypes`'s own field list, reached by reflection. A key the code
        // declares and the zip walk did not produce means the listing missed a
        // file, which is the failure this table could otherwise hide.
        List<String> declared = new ArrayList<>();
        for (Field f : DamageTypes.class.getFields()) {
            if (!ResourceKey.class.isAssignableFrom(f.getType())) {
                continue;
            }
            Object v = f.get(null);
            @SuppressWarnings("unchecked")
            ResourceKey<DamageType> key = (ResourceKey<DamageType>) v;
            declared.add(key.identifier().toString());
        }
        List<String> missing = new ArrayList<>();
        for (String d : declared) {
            if (!types.containsKey(d)) {
                missing.add(d);
            }
        }
        if (!missing.isEmpty()) {
            System.err.println("DamageTypes declares keys the jar walk did not find: " + missing);
            System.exit(1);
        }

        // ---- resolve every tag ----
        Map<String, Set<String>> resolved = new TreeMap<>();
        for (String tag : tagFiles.keySet()) {
            resolved.put(tag, resolve(tag, tagFiles, new LinkedHashSet<>()));
        }

        // ---- the tags the CODE declares, against the files that exist ----
        //
        // These are two different lists and they do not agree in 26.2:
        // `DamageTypeTags` declares a `TagKey` that has no data file, and a
        // TagKey with no file is simply an **empty tag** — every
        // `DamageSource.is()` against it is false, for every damage type, and
        // the branch it guards is dead. That is a fact about this version, it
        // is not visible from the bytecode of either side alone, and it is
        // exactly the kind of thing that would otherwise be discovered as a
        // "why does this never fire" a year later. So both lists are emitted
        // and the difference is named.
        List<String> declaredTags = new ArrayList<>();
        for (Field f : DamageTypeTags.class.getFields()) {
            if (!TagKey.class.isAssignableFrom(f.getType())) {
                continue;
            }
            Object v = f.get(null);
            @SuppressWarnings("unchecked")
            TagKey<DamageType> key = (TagKey<DamageType>) v;
            declaredTags.add(key.location().toString());
        }
        java.util.Collections.sort(declaredTags);
        List<String> tagsWithNoFile = new ArrayList<>();
        for (String t : declaredTags) {
            if (!resolved.containsKey(t)) {
                tagsWithNoFile.add(t);
            }
        }

        List<String> rows = new ArrayList<>();
        for (Map.Entry<String, JsonObject> e : types.entrySet()) {
            JsonObject o = e.getValue();
            StringBuilder r = new StringBuilder();
            r.append("    {").append(q("id")).append(": ").append(q(e.getKey()));
            r.append(", ").append(q("messageId")).append(": ")
                    .append(q(o.get("message_id").getAsString()));
            r.append(", ").append(q("scaling")).append(": ")
                    .append(q(o.get("scaling").getAsString()));
            // `exhaustion` and `effects` are optional in the codec; their
            // absence is the default, and the default is written out here so
            // a reader never has to know one.
            double exhaustion = o.has("exhaustion") ? o.get("exhaustion").getAsDouble() : 0.0;
            r.append(", ").append(q("exhaustion")).append(": ").append(exhaustion);
            r.append(", ").append(q("exhaustionBits")).append(": ")
                    .append(Double.doubleToRawLongBits(exhaustion));
            r.append(", ").append(q("effects")).append(": ")
                    .append(q(o.has("effects") ? o.get("effects").getAsString() : "hurt"));
            r.append(", ").append(q("deathMessageType")).append(": ")
                    .append(q(o.has("death_message_type")
                            ? o.get("death_message_type").getAsString() : "default"));

            List<String> in = new ArrayList<>();
            for (Map.Entry<String, Set<String>> t : resolved.entrySet()) {
                if (t.getValue().contains(e.getKey())) {
                    in.add(q(t.getKey()));
                }
            }
            r.append(", ").append(q("tags")).append(": [").append(String.join(", ", in))
                    .append("]}");
            rows.add(r.toString());
        }

        List<String> tagRows = new ArrayList<>();
        for (Map.Entry<String, Set<String>> t : resolved.entrySet()) {
            List<String> vals = new ArrayList<>();
            for (String v : t.getValue()) {
                vals.add(q(v));
            }
            tagRows.add("    {" + q("tag") + ": " + q(t.getKey()) + ", " + q("values") + ": ["
                    + String.join(", ", vals) + "]}");
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("damageTypes")).append(": ").append(types.size()).append(",\n");
        out.append("  ").append(q("declaredInCode")).append(": ").append(declared.size())
                .append(",\n");
        out.append("  ").append(q("tags")).append(": ").append(resolved.size()).append(",\n");
        out.append("  ").append(q("tagsDeclaredInCode")).append(": ").append(declaredTags.size())
                .append(",\n");
        List<String> noFile = new ArrayList<>();
        for (String t : tagsWithNoFile) {
            noFile.add(q(t));
        }
        out.append("  ").append(q("tagsWithNoFile")).append(": [")
                .append(String.join(", ", noFile)).append("],\n");
        out.append("  ").append(q("table")).append(": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ],\n");
        out.append("  ").append(q("tagTable")).append(": [\n");
        out.append(String.join(",\n", tagRows));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }
}
