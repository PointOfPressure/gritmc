// LinkedOrder — the three `Int2ObjectLinkedOpenHashMap` behaviours
// `EntityTickList` depends on, measured rather than assumed.
//
// `crates/rustmc-entity/src/tick_list.rs` reimplements that map as a `Vec`
// and states three semantics in comments. Comments are not measurements, and
// all three are the kind of thing a reasonable person gets backwards:
//
//   put-existing   does re-putting a key already present move it to the end,
//                  or leave it where it is?
//   remove-readd   does removing and re-adding move it to the end?
//   copy-order     `EntityTickList.ensureActiveIsNotIterated` rebuilds the
//                  passive map by walking the active one and putting each
//                  entry in. Does that preserve order?
//
// Prints one line per case: name, then the walk order.
import it.unimi.dsi.fastutil.ints.Int2ObjectMap;
import it.unimi.dsi.fastutil.ints.Int2ObjectMaps;
import it.unimi.dsi.fastutil.ints.Int2ObjectLinkedOpenHashMap;

public final class LinkedOrder {
    static void show(String name, Int2ObjectMap<Integer> m) {
        StringBuilder s = new StringBuilder(name);
        for (Integer v : m.values()) s.append(' ').append(v);
        System.out.println(s);
    }

    static Int2ObjectMap<Integer> of(int... ks) {
        Int2ObjectMap<Integer> m = new Int2ObjectLinkedOpenHashMap<>();
        for (int k : ks) m.put(k, Integer.valueOf(k));
        return m;
    }

    public static void main(String[] a) {
        show("baseline", of(1, 2, 3, 4));

        Int2ObjectMap<Integer> m = of(1, 2, 3, 4);
        m.put(2, Integer.valueOf(2));
        show("put-existing", m);

        m = of(1, 2, 3, 4);
        m.remove(2);
        m.put(2, Integer.valueOf(2));
        show("remove-readd", m);

        // The copy `ensureActiveIsNotIterated` performs, verbatim in shape:
        // fastIterable over the source, put each entry into a fresh map.
        Int2ObjectMap<Integer> src = of(4, 1, 3, 2);
        Int2ObjectMap<Integer> dst = new Int2ObjectLinkedOpenHashMap<>();
        for (Int2ObjectMap.Entry<Integer> e : Int2ObjectMaps.fastIterable(src))
            dst.put(e.getIntKey(), e.getValue());
        show("copy-source", src);
        show("copy-result", dst);
    }
}
