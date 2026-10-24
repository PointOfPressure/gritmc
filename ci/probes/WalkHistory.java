// WalkHistory — the walk order of `Int2ObjectOpenHashMap` after an arbitrary
// history of puts and removes, measured rather than reimplemented.
//
// `ci/probes/IterOrder.java` answers "what does this KEY SET walk as", which is
// the question roadmap §101.5 asked and the right one for a divergence report.
// It is not enough to grade a PORT. `removeEntry` halves the table when a map
// that has grown empties out, so two maps holding identical keys walk them
// differently if their histories differ, and an insert-only probe cannot see
// that at all. This probe takes the history.
//
// Reads one history per line on stdin. A token is `+k` for a put of key k with
// value k, or `-k` for a remove. Prints, per line:
//
//     <input>|<walk>|<object-keyed walk>
//
// where <walk> is the keys in `values()` order — the order `ChunkMap.tick()`
// broadcasts in, since it walks `entityMap.values()`.
//
// Nothing here is derived from any server implementation. It exercises
// fastutil, which is the library the pinned jar itself ships against, and it is
// run against that exact version, whose sha256 is recorded beside the golden it
// produces.
import it.unimi.dsi.fastutil.ints.Int2ObjectMap;
import it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap;
import it.unimi.dsi.fastutil.objects.Object2BooleanOpenHashMap;
import java.io.BufferedReader;
import java.io.InputStreamReader;

public final class WalkHistory {
    // `PlayerMap.players` is an `Object2BooleanOpenHashMap<ServerPlayer>`, and
    // `Entity.hashCode()` returns `getId()` with `equals` on the id. So the
    // player set is keyed on the same numbers as `ChunkMap.entityMap` — which
    // is the claim behind "one walk, two KEYHASH sites", and a claim is not a
    // measurement. This key has an entity's hashCode contract and nothing
    // else, so the third column measures whether the object map really does
    // walk the same ids in the same order as the int map. If it ever does not,
    // the check refuses rather than reporting a walk that is right for one of
    // the two sites.
    static final class IdKeyed {
        final int id;
        IdKeyed(int id) { this.id = id; }
        @Override public int hashCode() { return id; }
        @Override public boolean equals(Object o) {
            return o instanceof IdKeyed && ((IdKeyed) o).id == id;
        }
    }

    public static void main(String[] a) throws Exception {
        BufferedReader in = new BufferedReader(new InputStreamReader(System.in));
        StringBuilder out = new StringBuilder();
        String line;
        while ((line = in.readLine()) != null) {
            line = line.trim();
            if (line.isEmpty() || line.startsWith("#")) continue;
            // The no-argument constructor, which is how `ChunkMap.entityMap`
            // and `PlayerMap.players` are both built.
            Int2ObjectMap<Integer> m = new Int2ObjectOpenHashMap<>();
            Object2BooleanOpenHashMap<IdKeyed> o = new Object2BooleanOpenHashMap<>();
            for (String p : line.split("\\s+")) {
                int k = Integer.parseInt(p.substring(1));
                if (p.charAt(0) == '+') { m.put(k, Integer.valueOf(k)); o.put(new IdKeyed(k), true); }
                else if (p.charAt(0) == '-') { m.remove(k); o.removeBoolean(new IdKeyed(k)); }
                else throw new IllegalArgumentException("bad op: " + p);
            }
            out.setLength(0);
            out.append(line).append('|');
            boolean first = true;
            // `values()`, not the entry set, because `values()` is the exact
            // call `ChunkMap.tick()` makes. The value stored for key k is k,
            // so printing values prints the key order.
            for (Integer v : m.values()) {
                if (!first) out.append(' ');
                out.append(v);
                first = false;
            }
            out.append('|');
            first = true;
            for (IdKeyed e : o.keySet()) {
                if (!first) out.append(' ');
                out.append(e.id);
                first = false;
            }
            System.out.println(out);
        }
    }
}
