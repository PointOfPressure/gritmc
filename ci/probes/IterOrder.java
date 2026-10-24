// IterOrder — the walk order of the two fastutil maps the server's entity
// paths use, measured rather than reimplemented.
//
// `ChunkMap.entityMap` is an `Int2ObjectOpenHashMap` keyed by entity id and
// `ChunkMap.tick()` walks its `values()`, so that walk is the order every
// per-entity broadcast goes out in. `EntityTickList` and `EntityLookup` use
// `Int2ObjectLinkedOpenHashMap` instead, which walks in insertion order.
// Printing both for the same key set is what makes "keyed on the id values"
// and "keyed on insertion" a measurement rather than a claim.
//
// Reads one key set per line on stdin, as space-separated ints, in the order
// they are inserted. Prints, per line:
//
//     <input>|<Int2ObjectOpenHashMap walk>|<Int2ObjectLinkedOpenHashMap walk>
//
// Nothing here is derived from any server implementation. It exercises
// fastutil, which is the library the pinned jar itself ships against, and it
// is run against that exact version.
import it.unimi.dsi.fastutil.ints.Int2ObjectMap;
import it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap;
import it.unimi.dsi.fastutil.ints.Int2ObjectLinkedOpenHashMap;
import java.io.BufferedReader;
import java.io.InputStreamReader;

public final class IterOrder {
    public static void main(String[] a) throws Exception {
        BufferedReader in = new BufferedReader(new InputStreamReader(System.in));
        String line;
        while ((line = in.readLine()) != null) {
            line = line.trim();
            if (line.isEmpty() || line.startsWith("#")) continue;
            String[] parts = line.split("\\s+");
            Int2ObjectMap<Integer> open = new Int2ObjectOpenHashMap<>();
            Int2ObjectMap<Integer> link = new Int2ObjectLinkedOpenHashMap<>();
            for (String p : parts) {
                int k = Integer.parseInt(p);
                open.put(k, Integer.valueOf(k));
                link.put(k, Integer.valueOf(k));
            }
            StringBuilder s = new StringBuilder(line).append('|');
            boolean first = true;
            for (Integer v : open.values()) { if (!first) s.append(' '); s.append(v); first = false; }
            s.append('|'); first = true;
            for (Integer v : link.values()) { if (!first) s.append(' '); s.append(v); first = false; }
            System.out.println(s);
        }
    }
}
