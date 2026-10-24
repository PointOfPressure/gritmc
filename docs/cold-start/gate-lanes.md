# Cold start — branch `gate-lanes`

---

### COLD START — branch `gate-lanes` off `911c2e04`, 2026-08-16

**The brief's premise was refuted by the first measurement, and the refutation
is the main result.** The item asked for N lanes on the ground that "a correct
3-lane gate is still ~3x". It is not ~3x. It is **1.40x**, the ceiling does not
move with N, and N saturates at **2**.

#### What was measured, and how

The brief said "measure the serialised fraction, do not assume it". The
instrument is not a difference of medians: `ci/corpus-lock.sh`'s waiter
heartbeat prints the *holder's* directory and its acquisition timestamp, so for
any gate that some other process waited behind, the exact instant stage 6 took
the box-wide lock is recoverable from a third party's log. Six gates in
`~/rustmc-private/gates` have such a record against a gate directory.

    corpus lock HELD          38.4 min median   n=6   spread 36.3 .. 44.2
    stages 1-5 (rsync,
      tables, T0, T1)         15.5 min median   n=6   spread 14.8 .. 21.9
    ------------------------------------------------------------------
    serialised fraction       38.4 / 53.9 = 71%
    Amdahl ceiling            1.40x, at any N

Corroboration from the whole population, independent of those six: 118 full
gates (reaching `T2-GAMEPLAY GREEN`), median 50.0 min; split by whether they
waited on the corpus lock, **48.2 min for the 92 that did not** and 80.4 min
for the 26 that did (median wait 27.0 min, max 114). Classification of all 158
logs by furthest stage reached is in the commit message.

**N = 2 because the lock saturates there.** One lane is inside the 38.4-minute
lock; the other has 38.4 minutes of wall to do 15.5 minutes of stages 1-5 for
the next branch, and finishes with 23 minutes spare. A third lane has nothing
left to do and costs a third 7.2 GB build directory (measured on
`~/rustmc-stack`, 7.0 G of it `target/`), a third of the box's threads, and a
third set of vanilla JVMs. `-n` is a parameter so a later measurement can move
it; the default is the saturation point, not a preference.

#### The finding that matters more than the speedup

Of ~71 attempts in `gate-chain.log`, **21 merged**. The single largest loss is
**24 attempts refused with `another gate already owns the box directory
~/rustmc-stack`** — one third of the queue, spent in seconds, on a mutex over a
directory rather than on any property of a branch. Another **23 were `SKIP —
REBASE CONFLICT, needs hands`**, which is what a queue that does not drain
produces. Per-lane build directories delete the first outright. Draining faster
reduces the second. **1.40x on gate wall time is worth less here than not
throwing a third of the queue away**, and anyone re-opening this item should
start from that number rather than from the speedup.

Observed live while working (30 min sample, `lane-measure.sh`, 1/min): the
corpus lock was **free in 29 of 29 samples** with a gate in flight the whole
time, and the chain was turning branches over in ~8 s each, all
`rc=1 no MERGED/REFUSED/INFRA line`. The queue's binding constraint at that
moment was not lane count at all — it was branches failing T0 in seconds.

#### Why lanes without speculation are worth exactly zero

`ci/handoff.sh` refuses a branch that is BEHIND main and merges `--ff-only`.
Both are what make main's new tip *the tree the gate measured*. So two lanes
gating against the same base cannot both merge: the loser's whole gate is void
the moment the winner pushes. Retry logic does not change it. The only sound
overlap is to gate each branch against the tree it will actually land on —
a merge train. `ci/lanes.sh` is that.

    position 1: base = origin/main
    position 2: base = position 1's gated tip
    position 3: base = position 2's gated tip

The push race is therefore **eliminated rather than retried**: a lane merges
the exact commit it gated, only when `origin/main` is bit-for-bit its base,
with the check and the push inside one flock.

#### What a retry re-runs — the full gate, nothing scoped

Decided and justified in `ci/lanes.sh`'s header. Scoping to "what main's
incoming commits touched" is unsound here because several gate checks are
functions of the **union** and of neither side: `ci/check-doc-claims.py`
(a duplicate `## N.` reached main on 2026-08-12 exactly because a check read
one side and not the union), `ci/figures.toml` id uniqueness,
`t1-allowlist.py`'s `min_binaries`, and the ratcheted floors in
`ci/corpus-falsifiable.py`. A scoped retry needs a per-check input-closure map;
none exists, and one written by hand would under-scope **silently** rather than
fail — the defect class `docs/AGENT-BRIEF.md` forbids adding to.

#### Two real bugs my own design had, both caught by the sweep

* **The lane force-pushed its speculative rebase to `origin/<branch>`** (copied
  from `gate-chain.sh`, where it is correct). On a train that tip contains the
  *predecessor's* commits. When the predecessor went RED and the branch was
  re-admitted, `_admit` restarted from `origin/<branch>` — no longer the
  author's branch — so a **rejected branch rode along onto main**. The gated
  tip now lives only in the shared store as `refs/lanes/<seq>`; the author's
  branch is never rewritten.
* **Base assignment was not atomic with the rebase**, so two lanes admitting in
  the same instant both read the same `spec` and both gated against
  `origin/main` — the naive design that is worth zero. The rebase now happens
  inside admission under the state lock (seconds, against a 38-minute corpus
  run).

#### Nulls — do not re-run these

* **Do not look for a cheap scoped re-gate.** See above; it is unsound for a
  structural reason, not a tuning reason.
* **Do not raise N above 2 hoping for more throughput** without first
  re-measuring the lock-held fraction. At 71% serialised there is nothing there.
* **Do not give lanes private corpus-lock namespaces.** It is the obvious
  "optimisation" and it buys throughput by corrupting the reference every parity
  figure rests on. `ci/lanes-falsify.sh`'s `lock_bypass` injection exists to
  catch exactly that and does.
* **`_predecessors_done` is an efficiency measure, not a guard.** Removing it is
  caught by nothing, because the base check and git's own non-fast-forward
  refusal both cover it. Said out loud in the sweep rather than papered over
  with an injection that cannot be caught.

#### The proof

`ci/lanes-falsify.sh` — 6 cases, 4 injections, ~90 s, no box involvement.
`LANES_FALSIFY_DRYRUN=1` re-checks that every patch still matches after any edit
to `ci/lanes.sh`. **`red_does_not_ride_along` is the case that would catch a
regression** in the push race or in speculation soundness; `concurrency` catches
silent serialisation, `distinct_dirs` catches shared build directories, and
`lock_serialises` catches a lane routing around the corpus lock. Every timing
case refuses to report on fewer than two observed windows — the first version
passed `lock_serialises` on a drain that had run no gates at all, which is this
project's own defect class appearing in the instrument written to prevent it.

#### The single next item

**`ci/lanes.sh` has never driven a real `ci/handoff.sh`.** Everything above is
proven against a stand-in gate, deliberately, because a live chain owned
`~/rustmc-stack` throughout. The result that closes it: one two-lane drain over
two real branches with `RUSTMC_LANE_REMOTE_PREFIX=rustmc-lane`, showing two
distinct box directories, one `MERGED` per branch, and the second branch's base
equal to the first's gated tip in the drain log.
