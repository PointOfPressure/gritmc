# COLD START — `refcache-flag-proposal`

### COLD START — branch `refcache-flag-proposal`, 2026-08-18, area "gate throughput"

**What this branch is.** A decision record, not a code change. It names the one
line that turns `RUSTMC_REFERENCE_CACHE=1` on — `gate-chain.sh`'s own
`./ci/handoff.sh` invocation, nowhere else — and assembles everything an owner
needs to take that decision without looking anything up. **It does not flip the
flag and must not.**

**What changed on 2026-08-18: it went from "waiting on two things" to complete.**

**1. The prerequisite is met.** `refcache-root-invariance` merged as `8ce434d8`
on `origin/main` (chain log 17:30:06, T0+T1+T2 green). The document had said
"as of this document it is reported gating". Without it the cache key hashed
each scenario file's absolute path, so every tree on this box — all at distinct
`RUSTMC_REMOTE_DIR` paths by convention — would have stored entries no other
tree could ever key into.

**2. The live miss/hit number is in, and the obvious reading of it is wrong.**

**THE WALL TIMES SHOW THE CACHE BEING SLOWER, TWICE, AND THEY ARE NOT THE
INSTRUMENT.** miss 4875s; same-tree hit **5283s**; cross-tree hit 3004s; repeat
hit **5190s**. The probe ran beside the live production chain, so wall is
dominated by queueing for the corpus lock — pass 2 waited 66 minutes for the
lock where pass 1 waited 37. **A wall figure taken on a contended box is a
measurement of the queue.** Subtract each log's own
`corpus lock: ACQUIRED as gate after waiting Ns`:

    pass 1  MISS   4875s − 2277s = 2598s = 43.3 min
    pass 2  HIT    5283s − 3988s = 1295s = 21.6 min
                                  saving  21.7 min per gate

and 21.6 min lands on `docs/CORPUS-LOCK-HOLD.md`'s independently-made
prediction of ~21.6 min to the minute.

**3. The load-bearing result is not a timing.** `rustmc-refcache-bw-D`, at a
different absolute path from the tree that stored the entry, HIT that entry's
key, and the two trees' `vanilla.json` are byte-identical
(`d6e5285d389cd20288378acc814a490797b8a4b4477df57ff11d44b10e0aac6e`). A cache
that is fast and serves a *different* reference would move every gameplay
verdict on this box with nothing saying so. That, not the 21.7 minutes, is what
makes the flag a throughput change rather than a correctness change.

**The enable table — nine of ten hold.** Prerequisite merged; falsifier 24/24
with controls that must still hit; genuine cross-tree hit; byte-identical
artefact; both arms `T2-GAMEPLAY GREEN` at 108/108; measured saving against a
prediction made first; corrupt/truncated entries refused; default off and
opt-in per invocation; **disk growth 696 KB per entry** (so a thousand keys is
under 700 MB against 192 GB free — costed so nobody re-raises it). The tenth is
**the hit hold is N=1**: passes 3 and 4 were not logged separately so their lock
waits cannot be recovered. That is a sample size, not a doubt about the
mechanism, and it costs one T2 run as `role=run` to close.

**WHAT I KILLED AS A NULL — do not re-run.**
* **Do not quote, or try to reproduce, a wall-clock saving from this probe.**
  There isn't one and there cannot be while the chain is live. The instrument is
  `wall − (the log's own ACQUIRED-after-waiting line)`, and it is in the doc.
* **Do not re-litigate cache size.** Measured, 696 KB per entry. There is no
  eviction policy and at this size there does not need to be one.

**The single next item.** Either the owner flips the one line, or somebody takes
one more T2 run under `role=run` to make the hit hold N=2 and closes row 10 of
the table. Nothing else is outstanding. Note that this decision and batching's
are independent: batching was priced against the *uncached* hold, so enabling
the cache first makes batching's case better, never worse.
