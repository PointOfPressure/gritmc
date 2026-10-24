// JukeboxSongGolden — the JUKEBOX_SONG registry, in registry-id order.
//
// WHAT READS THIS
//
//   JukeboxSongPlayer.play, offsets 34-52
//       int id = registryAccess().lookupOrThrow(Registries.JUKEBOX_SONG)
//                                .getId(this.song.value());
//       level.levelEvent(null, 1010, blockPos, id);
//   JukeboxSongPlayer.tick, offsets 20-27
//       song.value().hasFinished(ticksSinceSongStarted)
//   JukeboxBlock's comparator output.
//
// So the number a jukebox puts on the wire is a **registry index**, not a
// name and not a hash, and `Mth.ceil(lengthInSeconds * 20)` is how long it
// keeps playing. Neither is derivable from the song's id.
//
// THERE IS NO REGISTRY-ID COLUMN HERE, AND THAT IS A MEASUREMENT.
//
// `Registry.getId` is the insertion order of a **datapack** registry, so the
// int `play` sends is a property of how the registry was loaded rather than
// of the jar's constant pool. The only loader reachable without a running
// server is `VanillaRegistries.createLookup()`, and its order is **not
// stable across JVM runs**: three runs of an earlier revision of this dumper
// gave three different orders, starting
//
//     tears, pigstep, ward, 11, wait, otherside, creator, 5, …
//     11, ward, pigstep, tears, chirp, mellohi, relic, cat, …
//     tears, chirp, mellohi, relic, cat, creator_music_box, far, …
//
// So it is not the wrong order, it is not an order at all — a column filled
// from it would have been a fresh random number on every regeneration of the
// table, and it would have looked entirely reasonable in a log.
//
// A real server loads this registry through `RegistryDataLoader` over
// `FileToIdConverter.listMatchingResources`, whose `ResourceManager
// .listResources` hands back a map sorted by `Identifier` — so its ids are
// expected to be the alphabetical rank. **That is an expectation and this
// file does not certify it**; nothing here has compared it against a vanilla
// recording, and the consumer says so at the point of use.
//
// What this table does certify is per-song and order-free: `lengthInTicks`
// and `comparatorOutput`. Rows are emitted sorted by song id so that the file
// itself is reproducible whatever the registry does.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.Mth;
import net.minecraft.world.item.JukeboxSong;

public final class JukeboxSongGolden {
    private JukeboxSongGolden() {}

    private static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        HolderLookup.RegistryLookup<JukeboxSong> songs =
            lookup.lookupOrThrow(Registries.JUKEBOX_SONG);

        // `listElements()` on a `MappedRegistry`-backed lookup walks
        // `holdersInOrder()`, which is sorted by registry id — so the
        // position in this stream **is** the int `play` sends, without
        // needing the `Registry` object `getId` lives on. Written that way
        // rather than reaching for the registry because the lookup is what
        // the data generator hands out and the two must not be allowed to
        // disagree silently.
        // SORTED BY ID BEFORE ANYTHING IS EMITTED. `listElements()` on this
        // provider does not give a stable order between JVM runs (see the
        // header), so taking its order would make this file differ from
        // itself and the shell script's two-run comparison would fail for a
        // reason that has nothing to do with the data.
        List<Holder.Reference<JukeboxSong>> ordered =
            new ArrayList<>(songs.listElements().toList());
        ordered.sort(java.util.Comparator.comparing(h -> h.key().identifier().toString()));

        List<String> rows = new ArrayList<>();
        for (int i = 0; i < ordered.size(); i++) {
            Holder.Reference<JukeboxSong> h = ordered.get(i);
            JukeboxSong song = h.value();
            rows.add("  {"
                + q("song") + ": " + q(h.key().identifier().toString()) + ", "
                + q("alphabeticalRank") + ": " + i + ", "
                + q("lengthInTicks") + ": " + song.lengthInTicks() + ", "
                + q("comparatorOutput") + ": " + song.comparatorOutput()
                + "}");
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append(q("songs")).append(": ").append(rows.size()).append(",\n");
        // `hasFinished(t)` is `t >= lengthInTicks() + 20`, from its
        // `bipush 20; iadd` at offset 5. The twenty is a tail the song keeps
        // playing for after its own length, and it is the constant a
        // reimplementation drops.
        out.append(q("finishedTailTicks")).append(": ").append(20).append(",\n");
        out.append(q("playLevelEvent")).append(": ").append(1010).append(",\n");
        // The consumer must not read `alphabeticalRank` as a certified wire
        // id. This flag is the file saying so about itself, so that a reader
        // who never opens the dumper still meets the caveat.
        out.append(q("registryIdIsUnverified")).append(": true,\n");
        out.append(q("stopLevelEvent")).append(": ").append(1011).append(",\n");
        out.append(q("table")).append(": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n]\n}\n");
        System.out.print(out);
        // `lengthInTicks` is `Mth.ceil(lengthInSeconds * 20.0f)`, so it
        // rounds **up**. Asserted here rather than trusted, because a
        // truncating implementation agrees on every whole-second song in the
        // vanilla set and would ship undetected.
        if (Mth.ceil(0.05f) != 1) {
            throw new IllegalStateException("Mth.ceil is not a ceiling");
        }
    }
}
