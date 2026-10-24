// IdHash2 — WHICH input does the identity hash depend on?
//
// arm A: 8 objects, hashed in allocation order 0..7.
// arm B: 8 fresh objects, hashed in the scrambled order 5,2,7,0,3,6,1,4,
//        printed back in ALLOCATION order.
//
// If the hash is a property of the object (address, allocation index), arm B
// printed in allocation order equals arm A. If it is drawn from the hashing
// THREAD's own sequence at first-hashCode time, then arm B's object #5 gets
// the same value arm A's object #0 got, and printing B in allocation order
// gives a permutation of A's values, not A's values.
public final class IdHash2 {
    static final int N = 8;
    static final int[] ORDER = {5, 2, 7, 0, 3, 6, 1, 4};

    public static void main(String[] a) {
        Object[] x = new Object[N];
        for (int i = 0; i < N; i++) x[i] = new Object();
        int[] ha = new int[N];
        for (int i = 0; i < N; i++) ha[i] = System.identityHashCode(x[i]);

        Object[] y = new Object[N];
        for (int i = 0; i < N; i++) y[i] = new Object();
        int[] hb = new int[N];
        for (int k = 0; k < N; k++) { int i = ORDER[k]; hb[i] = System.identityHashCode(y[i]); }

        StringBuilder s = new StringBuilder("A(alloc-order-hashed) ");
        for (int i = 0; i < N; i++) s.append(ha[i]).append(' ');
        System.out.println(s);
        s = new StringBuilder("B(scrambled-hashed,printed-alloc-order) ");
        for (int i = 0; i < N; i++) s.append(hb[i]).append(' ');
        System.out.println(s);
        s = new StringBuilder("B-in-DRAW-order ");
        for (int k = 0; k < N; k++) s.append(hb[ORDER[k]]).append(' ');
        System.out.println(s);
    }
}
