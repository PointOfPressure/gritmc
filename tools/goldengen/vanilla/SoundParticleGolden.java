// SoundParticleGolden — the three built-in registries and constant tables the
// sound and particle channels are numbered against.
//
// Three tables, one JVM run, because all three are a walk over a bootstrapped
// registry and the boot is the whole cost:
//
//   sound_events.json    every `BuiltInRegistries.SOUND_EVENT` entry, in id
//                        order, with its registry key, its sound location and
//                        its `fixedRange` — which is empty for all 1,968 in
//                        26.2; see below, that is a measurement.
//   particle_types.json  every `BuiltInRegistries.PARTICLE_TYPE` entry, in id
//                        order, with whether it carries options on the wire.
//   level_events.json    every `int` constant on `LevelEvent`, by name.
//   note_block.json      the `NoteBlockInstrument` enum: the sound event each
//                        one plays and the three predicates its `Type` decides.
//
// WHY THE RANGE IS A TABLE, AND WHAT MEASURING IT ACTUALLY SAID
//
// `ServerLevel.playSeededSound` offsets 41-46 pass `SoundEvent.getRange(volume)`
// to `PlayerList.broadcast` as the broadcast radius, and `getRange` is
//
//     fixedRange.orElse(volume > 1.0f ? 16.0f * volume : 16.0f)
//
// — `Optional.orElse`, so a fixed range wins whatever the volume is. This
// dumper was written expecting a handful of loud events to carry one.
//
// **NONE OF THE 1,968 DOES.** Measured 2026-09-05, three ways that agree:
// this dumper reports 0; `SoundEvents.<clinit>` makes 1,905 registrations and
// every one goes through a helper that calls `createVariableRangeEvent`, none
// through the private `register(Identifier, Identifier, float)` that would
// call `createFixedRangeEvent`; and across all 7,434 classes in the jar the
// string `createFixedRangeEvent` appears in exactly two constant pools,
// `SoundEvent`'s own and `SoundEvents`' unreachable helper. The mechanism is
// for **datapack** sounds, whose `fixed_range` arrives through
// `SoundEvent.DIRECT_CODEC`.
//
// So the column is empty and stays in, for three reasons that are not "it
// might be useful later":
//
//  1. The zero is asserted rather than assumed:
//     `ci/sound-particle-golden.sh` fails when `fixedRangeEvents` is anything
//     but 0, so a version that adds a fixed-range sound makes this generator
//     red and somebody read it, rather than quietly changing how far one
//     sound carries.
//  2. A zero that means "no event has one" and a zero that means "this dumper
//     cannot read the field" are the same number. `fixedRangeReadWorks` is
//     the positive control that separates them: it builds a fixed-range
//     event by hand and checks that `fixedRange()` and `getRange()` both see
//     it. Without that, this table's most load-bearing column would be
//     reporting an instrument failure as a fact about vanilla.
//  3. The other two columns — the id and the registry key — are what
//     `ci/sound-particle-golden.sh` cross-checks against Mojang's report, and
//     that check is this table's live work today.
//
// THE REGISTRY KEY AND THE SOUND LOCATION ARE TWO IDENTIFIERS, NOT ONE.
// `SoundEvents.register(Identifier key, Identifier location)` takes both and
// they are the same at nearly every site — but the two-argument overload
// exists because at some sites they are not, and `SoundEvent.location()` is
// the one a resource pack resolves while the registry key is the one the
// packet's varint numbers. This dumper emits both and counts the
// disagreements rather than assuming there are none.
//
// WHAT `simple` MEANS FOR A PARTICLE
//
// `ClientboundLevelParticlesPacket.write` ends with
// `ParticleTypes.STREAM_CODEC.encode(buf, particle)`, which is a
// `ByteBufCodecs.registry(PARTICLE_TYPE)`-dispatched codec: a varint of the
// type's registry id, then whatever that type's `streamCodec()` writes. For a
// `SimpleParticleType` that second part is empty, so the packet ends at the
// varint. `simple` is `value instanceof SimpleParticleType` — a fact about
// the jar, not a guess from the name — and it is the difference between a
// particle this server can send from the table alone and one that needs its
// own options encoder written.

package net.minecraft.world.level.levelgen;

import java.io.IOException;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;

import net.minecraft.SharedConstants;
import net.minecraft.core.Registry;
import net.minecraft.core.particles.ParticleType;
import net.minecraft.core.particles.SimpleParticleType;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.world.level.block.LevelEvent;
import net.minecraft.world.level.block.state.properties.NoteBlockInstrument;

public final class SoundParticleGolden {
    private SoundParticleGolden() {}

    private static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    /// Every float leaves here as raw bits beside its readable form, for the
    /// reason `SoundTypeGolden` does it: `0.9f` is not `0.9`, and a reader
    /// that parses the decimal has silently chosen a different number.
    private static String flt(String name, float v) {
        return q(name) + ": " + v + ", " + q(name + "Bits") + ": " + Float.floatToRawIntBits(v);
    }

    private static String soundEvents() {
        Registry<SoundEvent> reg = BuiltInRegistries.SOUND_EVENT;
        List<String> rows = new ArrayList<>();
        int fixed = 0;
        int keyDiffersFromLocation = 0;
        int size = reg.size();
        for (int id = 0; id < size; id++) {
            SoundEvent ev = reg.byId(id);
            if (ev == null) {
                throw new IllegalStateException("SOUND_EVENT has a hole at id " + id
                        + " — the registry is not densely numbered and every table"
                        + " keyed by id downstream is wrong");
            }
            Identifier key = reg.getKey(ev);
            String keyName = key == null ? "" : key.toString();
            String location = ev.location().toString();
            if (!keyName.equals(location)) {
                keyDiffersFromLocation++;
            }
            Optional<Float> range = ev.fixedRange();
            StringBuilder row = new StringBuilder();
            row.append("    {").append(q("id")).append(": ").append(id);
            row.append(", ").append(q("name")).append(": ").append(q(keyName));
            row.append(", ").append(q("location")).append(": ").append(q(location));
            if (range.isPresent()) {
                fixed++;
                row.append(", ").append(flt("fixedRange", range.get()));
            } else {
                row.append(", ").append(q("fixedRange")).append(": null");
                row.append(", ").append(q("fixedRangeBits")).append(": null");
            }
            row.append('}');
            rows.add(row.toString());
        }
        // THE POSITIVE CONTROL FOR THE COLUMN THAT IS EMPTY.
        //
        // Every one of the 1,968 reports no fixed range. That is the right
        // answer for 26.2 and it is indistinguishable from a dumper that
        // cannot read the field at all, so the field is read once off an
        // event built to have one. 42 is arbitrary; 0.5 is a volume below
        // 1.0, where the variable rule would give 16 and the fixed one must
        // still give 42.
        SoundEvent probe = SoundEvent.createFixedRangeEvent(
                Identifier.withDefaultNamespace("rustmc_probe"), 42.0F);
        boolean readWorks = probe.fixedRange().isPresent()
                && probe.fixedRange().get() == 42.0F
                && probe.getRange(0.5F) == 42.0F
                && probe.getRange(4.0F) == 42.0F;
        if (!readWorks) {
            throw new IllegalStateException("SoundEvent.fixedRange() cannot be read off an event"
                    + " built with createFixedRangeEvent — the empty column below would be an"
                    + " instrument failure reported as a fact about vanilla");
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("events")).append(": ").append(size).append(",\n");
        out.append("  ").append(q("fixedRangeEvents")).append(": ").append(fixed).append(",\n");
        out.append("  ").append(q("fixedRangeReadWorks")).append(": ").append(readWorks)
                .append(",\n");
        out.append("  ").append(q("keyDiffersFromLocation")).append(": ")
                .append(keyDiffersFromLocation).append(",\n");
        out.append("  ").append(q("table")).append(": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ]\n}\n");
        return out.toString();
    }

    private static String particleTypes() {
        Registry<ParticleType<?>> reg = BuiltInRegistries.PARTICLE_TYPE;
        List<String> rows = new ArrayList<>();
        int simple = 0;
        int overrideLimiter = 0;
        int size = reg.size();
        for (int id = 0; id < size; id++) {
            ParticleType<?> t = reg.byId(id);
            if (t == null) {
                throw new IllegalStateException("PARTICLE_TYPE has a hole at id " + id);
            }
            Identifier key = reg.getKey(t);
            boolean isSimple = t instanceof SimpleParticleType;
            if (isSimple) {
                simple++;
            }
            if (t.getOverrideLimiter()) {
                overrideLimiter++;
            }
            rows.add("    {" + q("id") + ": " + id
                    + ", " + q("name") + ": " + q(key == null ? "" : key.toString())
                    + ", " + q("simple") + ": " + isSimple
                    + ", " + q("overrideLimiter") + ": " + t.getOverrideLimiter()
                    + ", " + q("optionsClass") + ": " + q(t.getClass().getName()) + "}");
        }
        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("types")).append(": ").append(size).append(",\n");
        out.append("  ").append(q("simpleTypes")).append(": ").append(simple).append(",\n");
        out.append("  ").append(q("overrideLimiterTypes")).append(": ").append(overrideLimiter)
                .append(",\n");
        out.append("  ").append(q("table")).append(": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ]\n}\n");
        return out.toString();
    }

    /// `LevelEvent`'s constants, by reflection over the class rather than by
    /// transcription. The class is nothing but `public static final int`
    /// fields, so a version that adds one adds it here without anybody
    /// editing this file — which is the point, because the failure mode of a
    /// hand-copied id table is a number that is right for the version it was
    /// copied from.
    private static String levelEvents() throws IllegalAccessException {
        List<String> rows = new ArrayList<>();
        Field[] fields = LevelEvent.class.getDeclaredFields();
        int skipped = 0;
        for (Field f : fields) {
            int mods = f.getModifiers();
            if (!Modifier.isPublic(mods) || !Modifier.isStatic(mods) || !Modifier.isFinal(mods)) {
                skipped++;
                continue;
            }
            if (f.getType() != int.class) {
                skipped++;
                continue;
            }
            rows.add("    {" + q("name") + ": " + q(f.getName())
                    + ", " + q("id") + ": " + f.getInt(null) + "}");
        }
        if (rows.isEmpty()) {
            throw new IllegalStateException("no int constants on LevelEvent — the class moved,"
                    + " and an empty table here would read as 'this version has no level events'");
        }
        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("levelEvents")).append(": ").append(rows.size()).append(",\n");
        out.append("  ").append(q("skippedFields")).append(": ").append(skipped).append(",\n");
        out.append("  ").append(q("table")).append(": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ]\n}\n");
        return out.toString();
    }

    /// `NoteBlockInstrument`, which is the only thing standing between a note
    /// block's state and the sound it makes.
    ///
    /// `NoteBlock.triggerEvent` picks the event with
    /// `instrument.getSoundEvent()` and the pitch with
    /// `instrument.isTunable() ? getPitchFromNote(note) : 1.0f`, and
    /// `playNote` decides whether to queue the block event at all with
    /// `instrument.worksAboveNoteBlock() || level.getBlockState(pos.above()).isAir()`.
    /// All three predicates are one field — the private `Type` — and the enum
    /// itself is the only place that mapping exists:
    ///
    ///     isTunable            type == BASE_BLOCK
    ///     worksAboveNoteBlock  type != BASE_BLOCK
    ///     hasCustomSound       type == CUSTOM
    ///
    /// so `isTunable` and `worksAboveNoteBlock` are exact complements, which
    /// is worth emitting both of and asserting rather than collapsing: they
    /// are complements *because of how Type is used today*, and a fourth Type
    /// would separate them.
    ///
    /// `ordinal` is emitted and is **not** what anything on the wire uses —
    /// the `instrument` block-state property is a string. It is here because
    /// the enum's declaration order is the property's value order, and a
    /// reader that wants to check it has the whole enum can compare against
    /// it.
    private static String noteBlock() {
        NoteBlockInstrument[] all = NoteBlockInstrument.values();
        List<String> rows = new ArrayList<>();
        int tunable = 0;
        int custom = 0;
        for (NoteBlockInstrument in : all) {
            SoundEvent ev = in.getSoundEvent().value();
            Integer id = BuiltInRegistries.SOUND_EVENT.getId(ev);
            if (in.isTunable()) {
                tunable++;
            }
            if (in.hasCustomSound()) {
                custom++;
            }
            if (in.isTunable() == in.worksAboveNoteBlock()) {
                throw new IllegalStateException(in.getSerializedName()
                        + ": isTunable and worksAboveNoteBlock agree, and they are complements"
                        + " — NoteBlockInstrument.Type has changed shape");
            }
            rows.add("    {" + q("name") + ": " + q(in.getSerializedName())
                    + ", " + q("ordinal") + ": " + in.ordinal()
                    + ", " + q("sound") + ": " + q(ev.location().toString())
                    + ", " + q("soundId") + ": " + (id == null ? -1 : id)
                    + ", " + q("tunable") + ": " + in.isTunable()
                    + ", " + q("worksAboveNoteBlock") + ": " + in.worksAboveNoteBlock()
                    + ", " + q("customSound") + ": " + in.hasCustomSound() + "}");
        }
        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("instruments")).append(": ").append(all.length).append(",\n");
        out.append("  ").append(q("tunableInstruments")).append(": ").append(tunable)
                .append(",\n");
        out.append("  ").append(q("customSoundInstruments")).append(": ").append(custom)
                .append(",\n");
        // `NoteBlock.NOTE_VOLUME`, read rather than transcribed: it is the one
        // volume in this server above 1.0, which is the only volume at which
        // `SoundEvent.getRange`'s multiply fires at all.
        out.append("  ").append(q("noteVolume")).append(": ")
                .append(net.minecraft.world.level.block.NoteBlock.NOTE_VOLUME).append(",\n");
        out.append("  ").append(q("table")).append(": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ]\n}\n");
        return out.toString();
    }

    private static void write(Path dir, String name, String body) throws IOException {
        Files.writeString(dir.resolve(name), body, StandardCharsets.UTF_8);
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 1) {
            System.err.println("usage: SoundParticleGolden <output-dir>");
            System.exit(2);
        }
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        Path dir = Path.of(args[0]);
        Files.createDirectories(dir);
        write(dir, "sound_events.json", soundEvents());
        write(dir, "particle_types.json", particleTypes());
        write(dir, "level_events.json", levelEvents());
        write(dir, "note_block.json", noteBlock());
    }
}
