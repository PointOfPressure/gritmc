// IdHash — is HotSpot's identity hash reproducible across JVM runs?
//
// Written for this branch, from scratch. Nothing here is derived from any
// server implementation; it exercises the JDK only.
//
// Two arms, because "identity hash is deterministic" and "identity-hash
// iteration order is deterministic" are different claims and only the second
// is what an order-keyed behaviour depends on.
//
//   arm main   : N objects allocated in a fixed order on the main thread.
//   arm pool   : N objects allocated by 4 threads racing, joined, then read
//                back in allocation-completion order.
//
// Prints one line per arm: arm name, then the identity hashes.
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CountDownLatch;

public final class IdHash {
    static final int N = 8;

    public static void main(String[] a) throws Exception {
        List<Object> main = new ArrayList<>();
        for (int i = 0; i < N; i++) main.add(new Object());
        StringBuilder sb = new StringBuilder("main");
        for (Object o : main) sb.append(' ').append(System.identityHashCode(o));
        System.out.println(sb);

        Object[] pool = new Object[N];
        CountDownLatch go = new CountDownLatch(1);
        Thread[] ts = new Thread[4];
        for (int t = 0; t < 4; t++) {
            final int tt = t;
            ts[t] = new Thread(() -> {
                try { go.await(); } catch (InterruptedException e) { return; }
                for (int i = tt; i < N; i += 4) pool[i] = new Object();
            });
            ts[t].start();
        }
        go.countDown();
        for (Thread t : ts) t.join();
        StringBuilder sp = new StringBuilder("pool");
        for (Object o : pool) sp.append(' ').append(System.identityHashCode(o));
        System.out.println(sp);
    }
}
