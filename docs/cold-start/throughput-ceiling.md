# COLD START — branch `throughput-ceiling`

### COLD START — branch `throughput-ceiling`, 2026-08-19

The area was "land the stranded tooling, turn batching on, and make the
reference cache earn its place". Three of those four resolved against the
brief's own expectation, and every reversal is a measurement rather than an
opinion.

## 1. The three stranded scripts are landed, and the reason they were stranded

`ci/run-detached.sh`, `ci/unlanded.sh` and `ci/claim-number.py`, with their
falsifiers, are on this branch and gated in `ci/t0.sh`.

**They were not merely on the wrong branch. All three are listed in
`.git/info/exclude`** — lines 13, 14 and 15, added 2026-08-18 23:27, alongside
`ci/corpus-grades.py` and `ci/corpus-history.py`. That file is **not
per-worktree**: all ~250 `~/grit/work-*` trees are worktrees of one repository
and share one `.git`, so the entry applied to every lane at once. A lane that
copied a script in and ran `git add` was told its own script was ignored, and
if it was not reading closely it committed without it. `git add -f` was needed
to land them. Once a path is tracked the exclude entry is inert for it, so
nothing else had to change and no other lane's tree was disturbed.

**Two of `ci/claim-number-falsify.sh`'s cases failed for being right about a
box that has moved**, and both are now deltas rather than levels:

* case 1 asserted that removing its §54 poison leaves `--check` GREEN. On main
  it does not, because two genuine unrepaired cross-branch collisions exist —
  **DETERMINISM §46** (`clamp-does-not-clamp-the-nether` against
  `worldgen-aggregate` on `land-train`, `probe-wgl`, `worldgen-land`) and
  **PARITY-ROADMAP §57** (`nether-is-two-bastions` against
  `serverbound-protocol-layer` on `corpus-step-respawn`,
  `corpus-step-vocabulary`, `proto-layer`). Neither is repairable from main's
  tree; the branches have to renumber. The case now snapshots the collision set
  before poisoning and requires it back exactly.
* case 8 asserted no new ref appeared in `refs/claims/`. Two did, inside the
  run: PARITY-ROADMAP §104 `parallel-serial-stages` and §105 `extractability`,
  both at 17:37, both another lane allocating, which is what the store is for.
  It now identifies leaks by the blob rather than by set difference.

23 passed, 0 failed after that.

**`ci/claim-number.py --check` is deliberately not in T0**, though
`origin/claims-allocator` gated it. It would red T0 for every lane on the box
today over the two collisions above, which no lane can fix from its own tree.
A ratchet on the count is worse: the count is a function of all refs, so
another lane pushing a branch would red your gate. The script's own docstring
already said `--check` is a pre-flight and not a gate; that branch gated the
thing its author documented as not a gate.

## 2. Batching: NOT enabled, and the reason changed twice while measuring

**The rate criterion cannot be evaluated at all.** `ci/gate-rate.py --decide`
answers "the criterion is NOT met under any definition". `~/rustmc-private/gates`
holds **28 logs, all stamped 2026-08-19, oldest 01:05**; 11 reach a verdict,
giving **p = 4/11 = 0.3636** against a `MIN_N` of 20. The **p = 0.8217 at
N = 157** in `docs/ASSESSMENT-2026-08-19.md` is not reproducible on this box,
because the logs it was measured from are gone. Nothing is wrong with the gate;
the evidence store was emptied. Re-accumulating it costs calendar and no
tokens.

**The unit was wrong, and fixing it inverts the recommendation.** Every figure
in `ci/batch.sh`'s width table is in gates, which prices a RED and a GREEN the
same. On the real 60-branch GREEN queue, planned by
`ci/batch.sh --plan --width 8 --on-red halves` into groups **5,6,5,5,8,4,4,4**
with **19 EXCLUDED-COLLISION**:

|            | counting gates | priced 65.2 / 3.4 |
|------------|----------------|-------------------|
| singles    | 1.269x         | 1.446x            |
| bisect     | 1.229x         | **1.725x**        |

The winner inverts with the unit, and the assessment's 1.725x is bisect's
number at width 8 — not the shipped default's. Mean admitted group is **5.12**
against a requested 8, and that gap is the entire distance from the nominal
2.221x to 1.725x: it is collision structure, exactly as the assessment says.

Getting that table out required repairing the instrument. `--groups` was
**silently dropped** whenever `--cost-green/--cost-red` were supplied (the cost
branch returned early), and `observed()` **hardcoded `batch_singles`** so it
could not score bisect at all, and priced every gate at a flat `80 min`.
Selftest C6-C9 pin the inversion; poisoned by restoring the hardcoded
strategy, C7 and C8 go red.

**And then today's own costs killed it.** `ci/gate-rate.py --cost` on the 28
surviving logs reports **GREEN 107.9 min (n=4)** and **RED 26.1 min (n=7)** —
a RED at 0.242 of a GREEN, not the 0.052 that made a wide batch pay. At those
costs **no width beats serial at any strategy**. On medians (115.3 / 4.9, the
RED mean being skewed by one T2 RED at 115.4) the best is **width 2 singles at
1.139x** — nowhere near width 8 bisect.

So: not enabled, and the honest reason is not caution. It is that the cost
asymmetry the whole case rests on is not present in the only gate logs that
exist.

**WHAT WAS NOT RUN, SAID PLAINLY.** The brief asked for a live run at width 8
/ bisect before enabling. What ran is `ci/batch.sh --plan --width 8 --on-red
halves` against the real queue — real cherry-pick feasibility, real group sizes,
real collisions — and **not a live batch gate**. That run is a precondition for
*enabling*, and enabling is refused on the costs above, so paying two hours of
box time to confirm a configuration the arithmetic already rejects was the wrong
trade tonight. It is precondition 3 below and it is the owner's in any case,
because `--land` advances `main`. Read every batching figure here as arithmetic
over a measured plan, not as a gated batch.

**Three of the four preconditions in `ci/batch.sh` now hold**, which is the
part that is worth keeping:

1. **DONE tonight.** `ci/handoff.sh` now takes the main-writer mutex around its
   merge step and **fails closed** — refuses with exit 2 rather than merging
   unprotected. `ci/batch.sh` had taken that lock since it was written and
   handoff never did, so it excluded nobody, which is worse than no lock
   because the party that takes it believes it is protected.
   `ci/handoff-merge-tail-falsify.sh` **case 13** holds the lock from another
   process and asserts the refusal, that the refusal names the mutex, and that
   nothing is merged; its control requires MERGED on a free lock. 37 passed,
   0 failed (was 33).
2. **DONE, and `ci/batch.sh` was stale about it.** It asserted "No `git fetch`
   exists anywhere in `ci/handoff.sh`", checked 2026-08-17. One has been added
   since, with explicit handling of local `main` behind, ahead of, and diverged
   from `origin/main`.
3. **STILL OPEN and it is the owner's.** One live width-2 `--land` run.
   `--land` advances `main`; not an agent's to take.
4. **DONE.** `ci/batch-falsify.sh` — 54 passed, 0 failed, 0 injections not
   caught. (The header said "24 cases and 9 injections".)

`P_FLOOR = 0.70` is derived from the gate-unit break-even of 0.6934. Swept in
minutes at 65.2/3.4 the break-even is between **0.20 and 0.25**, so the floor
is about three times too strict. **Deliberately not changed**: it guards the
owner's `--land` switch and the rate it compares against cannot currently be
measured. Loosening an enable gate on an unmeasurable input is the wrong order.

## 3. The reference cache: per-delta REFUSED on its own numbers, and not deleted

`ci/refcache-reuse.py` re-derives all of this in seconds from git alone.

The assessment's stated cause — "every branch edits scenarios" — is mostly
false. Over the 60 GREEN rows against 108 scenarios on main:

* **40 of 60 (67%) touch ZERO scenarios**; mean 1.98, median 0; only three
  touch more than 7;
* **38 of 60 (63%) move none of scenarios, harness or recorder** — these should
  already hit under the key exactly as it stands.

**`main` is what invalidates the cache, not the branches.** Every branch
rebases onto a moving main, and over the seven days to 2026-08-19 across 350
commits main moved harness 9 times, recorder 8, scenarios 5 — **22
invalidations, about one every 7.6 hours** against a gate of over an hour. An
entry survives roughly six gates before being evicted for every tree at once,
and since each tree sits on a different main tip each computes a key no other
tree will ever compute again. The live store agrees: six entries, `jar` and
`seed` constant, `tables` two values, **`harness` three** — two entries differ
in `harness` alone.

**The estimate that was asked for:** per-delta addresses the scenarios
component and nothing else — **5 of 22 weekly invalidations, 23%**. The other
77% invalidate all 108 scenarios at once for a structural reason: a recorder
change alters how every scenario is recorded, so no slicing of the key reaches
them. Building it is building the smaller half of the fix and measuring no hits
at the end. **Refused.**

**Not deleted either.** The key is fail-closed and that property is correct and
load-bearing: any unreadable component prints `MISSING` and lookup refuses
rather than treating it as a match, which is why a stale serve has never been
possible. What does not pay is `harness`, and it is repairable — it is three
whole files concatenated, one of which is `ci/reference-cache.sh` **itself**,
so editing a comment in the cache evicts everything the cache holds, and
`ci/t2-gameplay.sh`'s lock handling and verdict strings cannot affect what
vanilla records but evict the store when they change.

**The next item, and the result that closes it:** narrow `harness` to a
declared format version plus the functions that actually produce a recording,
with cases in `ci/reference-cache-falsify.sh` proving the narrowed key still
refuses a recording taken under a harness that could have changed it, and the
hit rate instrumented. Closes when the hit rate over a week is a number above
zero. **Do not attempt it without the falsifier cases first** — narrowing a
cache key is precisely the change that turns "never hits" into "serves a stale
reference".

## 4. The corpus lock, PREPARED AND NOT FLIPPED — owner's decision

The change is two acquisition sites, not one line, and saying "one line" would
be wrong:

```diff
--- a/ci/corpus-lock.sh
+++ b/ci/corpus-lock.sh
@@ _cl_wait_run_lock
-_cl_wait_run_lock()  { eval 'flock -x "$_CL_RUN_FD"'  "$_CL_GATE_FD>&- $_CL_WAIT_FD>&-"; }
+# role=run takes it SHARED so runs proceed together; role=gate keeps -x, which
+# is what still makes a gate exclusive against every run.
+_cl_wait_run_lock()  { eval "flock -${_CL_RUN_MODE:-x} \"\$_CL_RUN_FD\"" "$_CL_GATE_FD>&- $_CL_WAIT_FD>&-"; }
```

and, in `corpus_lock_acquire`'s non-gate fast path, `flock -x -n "$_CL_RUN_FD"`
becomes `flock -s -n "$_CL_RUN_FD"`, with `_CL_RUN_MODE=s` set for `role=run`.
Gates are untouched: they keep `flock -x` on the same fd, so gate-excludes-runs
and gate-excludes-gate both survive unchanged.

**The evidence for it, from the measurements that founded this:** concurrency
at **at most 1.3% slowdown** (95% CI 0.994–1.013, N=16 per arm), integrity
**0/8 in every arm** across 240 pairwise comparisons of 32 byte-distinct
recordings, and the incidents that founded the lock had **per-tree ports
already**. Exclusivity is unexplained rather than justified, and costs about
**450 agent-minutes per occurrence**.

**It is not flipped here and an agent should not flip it.** Being wrong
corrupts the reference every number in this repo rests on.

**The falsifier rework, costed by reading the suite rather than from the
brief.** `ci/corpus-lock-falsify.sh` has **23 cases and 19 injections**
(measured: `run_suite` calls 23; `CORPUS_LOCK_FALSIFY_DRYRUN=1` applies 19
patches). The brief costed it against 33 existing cases; the file says 23 in
its own header and 23 is what it runs.

* **13 survive unchanged** — they assert gate-vs-run priority, anchor liveness
  or refusal, none of which the change touches: `gate_priority`,
  `gates_serialise`, `kill9_frees`, `kill9_with_live_server`,
  `child_cannot_inherit`, `dead_anchor_refused`, `anchor_failure_refuses`,
  `stale_record_is_pid_not_timeout`, `observable`, `waits_not_fails`,
  `gate_beats_the_queue`, `two_gates_beat_the_queue`, `post_acquire_hand_back`.
* **8 need rework** because their blocker is another *run* and must become a
  *gate*: `gate_beats_crowd`, `kill9_after_waiting`, `dead_waiter_reaped`,
  `dead_waiter_no_stall`, `queue_survives_a_death`, `burst`, `fifo_no_barge`,
  `fifo_service_order`.
* **1 inverts and becomes the positive control**: `case_exclusion` today
  asserts four runs serialise; it must assert they overlap, and it is then the
  only case proving the change took effect.
* **1 must be re-derived**: `anchor_is_sole_holder` — under shared mode many
  processes hold the lock, so "exactly one holder" becomes "exactly one anchor
  per holder".
* **6 cannot exist today** and are the new obligations the change creates:
  1. N concurrent runs must not write into each other's recordings;
  2. their recordings must be byte-distinct and each internally valid — the
     0/8 integrity result, mechanised;
  3. a gate arriving mid-flight must wait out **all** N runs, not one;
  4. a run must still refuse to start while a gate holds;
  5. `status` must list **every** current holder, not one;
  6. `kill -9` of one of N holders must **not** free the lock for a gate while
     the others still run.

**Total 29 cases** (23 existing, of which 10 are rewritten or inverted, plus 6
new) and the 19 injections need re-checking against the patched library because
a patch that has stopped matching is a hole in the sweep. Budget the full
17-minute run, not the 2-second dry run.

## 5. The drain rate, RE-MEASURED — it is not 22 a day

`docs/ASSESSMENT-2026-08-19.md` §4.1 puts the ceiling at `1440 / 66.1` ≈ **22
branches a day**. Re-measured on the gate logs that exist:

| basis | p | GREEN | RED | min/landed | branches/day |
|---|---|---|---|---|---|
| assessment | 0.8217 (N=157) | 65.2 | 3.4 | 65.9 | **21.8** |
| measured, means | 0.3636 (N=11) | 107.9 (n=4) | 26.1 (n=7) | 153.6 | **9.4** |
| measured, medians | 0.3636 (N=11) | 115.3 | 4.9 | 123.9 | **11.6** |

**None of these three rows is registered in `ci/figures.toml`, deliberately.**
Neither is the assessment's 22, so nothing is retracted by adding this. `ci/gate-rate.py` refuses to call anything below N=20 a rate, and a figure registered at N=11 would be a number carrying a provenance and a floor it cannot support — the shape this repo keeps having to retract. Register it when the log store is back above N=20, and register the cost pair beside it, because the drain rate is a function of all three.

**The ceiling on landed work today is 9 to 12 branches a day, not 22**, and it
does not move with lane count. Every N here is small — 4 GREEN gates and 11
verdicts — so quote it as an estimate with its N, exactly as `ci/gate-rate.py`
refuses to call it a rate below N=20. It is nonetheless the only evidence on
the box, and it points the same way under both the mean and the median basis.

### And what that ceiling means for the backlog

`ci/unlanded.sh`, run at full scale the hour it landed — which is the first
time the instrument and the number have been in the same tree:

```
UNLANDED against origin/main (e2fc07600), 296 branches
    4434  sum of rev-list --count per branch   OVERSTATES
    2922  union, de-duplicated by sha          still counts rebased-and-landed
    2625  git cherry, distinct shas            not on main by patch-id
    1444  distinct patch-ids                   THE BACKLOG
```

`docs/ASSESSMENT-2026-08-19.md` §3.6 measured 1,400 over 292 branches earlier
the same day. **It is 1,444 over 296 now** — the backlog grew by 44 patch-ids
and four branches while this lane worked, which is the assessment's own
"production outruns landing" observation happening in real time.

Draining 296 branches at the re-measured rate is **26 to 32 days** of
continuous gate time, not the 13 the assessment's 22-a-day gives. And the
independent corroboration is the simplest possible one: `origin/main` was at
**2324 when this lane started at 18:35 and still 2324 at 19:20**, with eight
to ten lanes running. Zero branches landed in forty-five minutes. That is one
observation and not a rate, but it is the same direction as every number above.

## What was killed as a null, explicitly, so nobody re-runs it

* **Per-delta reference cache keying.** Refused on measurement: 23% of
  invalidations, cannot lift the hit rate. Do not build it. §3 above.
* **Enabling batching at width 8 / bisect.** At the costs measurable today no
  width beats serial; on medians the best is width 2 at 1.139x. The width-8
  optimum is real only at the cost ratio 65.2/3.4, which today's logs do not
  show. Re-open only with N ≥ 20 verdicts in `~/rustmc-private/gates`.
* **Deleting the reference cache.** Its fail-closed key is correct; the
  `harness` component is what does not pay, and that is repairable.
* **Gating `ci/claim-number.py --check` in T0.** It would red every lane over
  two collisions no lane can repair. A ratchet on the count is worse.

## The single next item

Re-accumulate gate verdicts to N ≥ 20 in `~/rustmc-private/gates` — that is
calendar, not work — then re-run `ci/gate-rate.py --decide` and
`ci/batch-arith.py --groups $(ci/batch.sh --plan ...) --cost-green G
--cost-red R`. **It closes when p and the two costs are measured at N ≥ 20 and
the priced table either does or does not beat serial.** Everything else in this
area is already built; the only missing input is a rate.
