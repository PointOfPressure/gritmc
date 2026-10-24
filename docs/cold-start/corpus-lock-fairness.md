# Cold start — branch `corpus-lock-fairness`

---

### COLD START — branch `corpus-lock-fairness`, off `main` (`3ddb3ca8`), 2026-08-12

**What this branch is: `ci/corpus-lock.sh` is now FIFO among runs, gate priority
unchanged. No Rust, no scenarios, no corpus measurement, no figure quoted.**
Worktree `~/grit/work-fair`, remote dir `~/rustmc-fair`. Not merged — the
orchestrator gates.

## The gate

`./ci/corpus-lock-falsify.sh` — **CORPUS-LOCK-FALSIFY GREEN, 47 cases passed, 0
failed, 0 injections not caught**, 16m42s, Threadripper, 2026-08-12. That is 23
cases run twice (control before and after the sweep) plus the guard self-test,
against 19 injections each of which had to turn a named case red.

`./ci/t0-remote.sh` on the committed tree in `~/rustmc-fair` — **T0 GREEN**,
`grep -c "skipping: no vanilla"` = 0. No corpus run: this branch touches neither
`ci/checks/` nor `crates/rustmc-server/`, and it takes no corpus measurement, so
no figure is quoted anywhere in it.

The four properties and the injection that removes each:

| property | injection | case that goes red |
|---|---|---|
| an older waiting run acquires before a newer one | `no-fifo-ordering` (`_cl_older_run_ahead` reports nobody) and `fast-path-barges-past-the-queue` (`_cl_any_live_waiter` reports nobody) | `fifo-no-barge`, `fifo-service-order` |
| a gate still preempts both | `no-gate-yield` (`_cl_gate_declared` never fires), `gate-declares-no-intent` | `gate-priority`, `gate-beats-crowd`, `gate-beats-the-queue`, `two-gates-beat-the-queue` |
| no livelock, deadlock or lost wakeup under a burst | `yield-to-any-run-not-just-older` (order stops being antisymmetric), `park-while-holding-the-mutex`, `park-does-not-block` (yield polls instead of blocking) | `burst`, `fifo-service-order`, `post-acquire-hand-back` |
| a killed waiter does not stall the queue | `dead-waiter-blocks-the-queue` (both liveness answers removed at once) | `dead-waiter-no-stall` |

## The measurement that opened it

Threadripper, 2026-08-11 20:05, `~/.rustmc-corpus-lock/`:

```
holder:  role=run   tree=~/rustmc-stackdec  since 19:42
waiters/ 2454442    role=run   tree=~/rustmc-sound   since 18:07   <- 1h58m
         2766884    role=gate  tree=~/rustmc-stack   since 20:01
```

`gameplay-sound-coverage` had been queued 1h58m and had lost the box to at least
three later arrivals. It cannot be gated until its subset-stability ensemble
runs, so the wait was holding it out of every merge chain.

## The finding that matters, and it is not where the item expected it

**The overtaking was in the fast path, not in the kernel's choice of which
blocked waiter to wake.** `flock` releases by waking waiters to re-contend, so a
blocked waiter and a fresh arrival race — but the fresh arrival was not even
racing. `corpus_lock_acquire`'s uncontended path takes the mutex with
`flock -x -n`, which on a free lock always succeeds; that path never blocks, so
it never registered a waiter and never looked at `waiters/` at all. Every time
the box came free, whoever happened to be starting in that instant walked past a
queue it could not see. Not a rare race — a certainty, every time, repeated
every fifty minutes.

That is why `case_fifo_no_barge` is deterministic rather than a repeated race:
it constructs the exact state (an older run queued, mutex free) and the old code
fails it 100% of the time.

## What changed

Each queued process holds an **exclusive `flock` on its own `waiters/<pid>`
record** for exactly as long as it is queued. That token is the whole protocol.

* A run finds the oldest run queued ahead of it and **blocks on that record's
  token** before contending at all, so the mutex is contended by one run at a
  time and the kernel's arbitrary wakeup stops deciding who goes next.
* A run that takes the mutex anyway (an older run queued while it was blocked)
  hands it straight back — the same hand-back that already existed for gates,
  one priority level down. It has spent nothing at that point.
* The fast path asks whether anybody is queued, before taking the mutex and
  again after. **This is the line the 20:01 arrival went through.**
* Ordering key is a new `seq` field (nanoseconds) then `pid`. `since` keeps its
  seconds and its meaning for humans.
* Gates consult no queue and hold no position in one. Priority stays absolute.

**No userspace liveness was added, and that is the point of the token.** It is an
`flock` on an fd, exactly like the main lock, so the kernel drops it when the
process dies — no ticket numbers, no sweeper, no timeout, nothing to tune.
Measured: a parked process was woken **0 ms** after its target was SIGKILLed
(`case_dead_waiter_no_stall`, "got in after 1s" is the 1-second polling grain of
the harness, not the latency).

### An entry whose process died — three independent answers

1. **Its token is already free.** The kernel released it on death, so parking
   behind a corpse returns immediately. Load-bearing, and it needs no code.
2. **`pid` + `pidstart` liveness**, as for the holder record. `pidstart` is why a
   recycled PID cannot impersonate one; this is the first code to read that field.
3. **The kernel is asked directly** (`_cl_record_is_queued`) whether a candidate
   still holds its own token, before anybody parks behind it.

Nothing is a timeout on a file's age. Either of (2) and (3) alone catches a dead
waiter, which is why the injection that tests the claim removes **both** —
same shape as the existing `both-anchor-guards-removed`.

### Why it cannot livelock

Structural, not careful. The order is `seq` then `pid`, **both read from files
and both fixed at registration**, so it is a strict total order that is
antisymmetric and *stable* — A and B compare the same way every time either
asks, whatever the clock does afterwards. Any non-empty set of queued runs
therefore has exactly one minimum, and that one yields to nobody. Yields
**block** on a token rather than looping, so there is no retry cadence anywhere
and nothing to tune; the wait-for graph follows the same total order and is a
DAG. `run.lock` is released before parking, never held across it — that one line
is the only ordering constraint the queue adds, and injection 17 removes it.

## The injection that came back MISSED, which is the most useful thing here

`park-while-holding-the-mutex` — remove the one line that releases the mutex
before parking — was **not caught** on the first sweep, against a case list of
`fifo-service-order` and `burst`. The suite was right and the reason is
structural: **no case could reach that branch.** The pre-contend park means a
run at the back of the queue never touches the mutex, so holding the mutex and
*then* finding somebody ahead of you only happens when they registered while you
were blocked — `seq` is taken at the top of `_cl_register_waiter` and the record
is published a rename later, so two processes registering together can be
published in the opposite order to their places. Sub-millisecond, and nothing
outside can aim at it.

`case_post_acquire_hand_back` constructs the state instead of racing for it: a
fixture registers normally (real token, live PID, matching `pidstart`), rewrites
its own record **in place** with the oldest `seq` there is, then blocks on the
mutex. `>` truncates the inode so the flock survives; **`sed -i` would have
written a new file and silently dropped the token**, and the case would have
passed while testing nothing. Control green in 14s, injection CAUGHT.

The lesson to carry: an injection whose case list is "everything that sounds
related" is how a hole gets reported as a pass. Injection 17 now names one case.

## Also cost a gate cycle: the remote-setup snippet has no PATH

`docs/AGENT-BRIEF.md`'s remote-setup block was missing the `export
PATH=$HOME/.cargo/bin:$PATH` that its own inner-loop block has. A non-interactive
`ssh` gets no login profile, so `ci/gen-gameplay-tables.sh` ran eleven generators,
got most of the way, and died at `ci/dump-recipe-unlocks.sh` with `cargo: command
not found` — **after** `cp -rn` had brought the shared tables in and **before**
they had all been restamped. T0 then came back red on
`ci/check-use-probe-falsify.sh` (`the dumper swept 1 stances, ... pins 12`, three
STALE rows) in a tree whose branch touches no Rust at all. Fixed in AGENT-BRIEF.

## The rollout hazard, which is real

Trees on this box do not all update at once. A waiter record written by the
pre-2026-08-12 library has **no token**, so parking on it would return instantly
and spin. Records carry `fairv=1` and only those are queued behind; the rest are
treated exactly as they are today. **If this is reverted or an old tree writes
records, nothing spins — it degrades to the old behaviour.**

## What is NOT claimed

* FIFO among processes **registered as queued**. A run arriving at a genuinely
  idle lock with nobody queued takes it at once — the uncontended case, not an
  overtaking.
* The fast path's two queue checks bracket a `flock -x -n`, so an arrival landing
  in the microseconds between an older run registering and the second check can
  still take the mutex **once**, and then hands it straight back having spent
  nothing.
* No corpus figure is quoted here and none was taken. (`proj:` is moving from
  `7cefde0fb56d9f3e` to `67604e5eb262728f` on another branch; floors stamped with
  the old id are stale.)

## Also fixed: `status` was slower than the run it was measuring

**`_cl_fd_holders` took 12.2 s and now takes 0.111 s. Same answer, 110x.** It
forked a `readlink` per fd per process — 627 processes on this box. It was not
merely slow, it made the function *wrong to call*: `status` gathered its sections
over fourteen seconds, so it could print a live holder, then an empty fd list,
then an empty waiter list, for one run that was alive throughout and had simply
ended partway down its own output. That cost most of an hour here, read at first
as a bug in the new queue.

`[ "$link" -ef "$target" ]` is a shell builtin comparing device and inode, so no
fork — and comparing inodes is also **more correct** than comparing the link
text, because `/proc/<pid>/fd/N` for an unlinked file reads as `path (deleted)`
and would never string-match. `status` also samples the queue before the scan
now; that was half the fix and stays, but the scan was the half that was wrong.

**This was pre-existing and is not the queue's doing — the queue's own
falsification sweep is what caught it**, by putting `anchor-sole` red (`0
processes hold the lock fd, want exactly 1`) for a lock that was demonstrably
held. The case holds for 8 s and was interrogating a 14 s instrument. The
tempting fix was to lengthen the case's hold, which would have hidden a defect
in the thing every operator uses to answer "who has the box".

## Nulls — do not re-run these

* **A ticket queue was not built and should not be.** The header's original
  objection to one was right: tickets need a rule for a dead ticket-holder, and
  every liveness check invented in this repo has eventually been found to be
  measuring itself. The token *is* the place in the queue, so there is nothing
  to reap.
* **Do not make the ordering key monotonic-clock-based.** It does not need to
  be: the comparison is between two stored values, so a backwards clock step can
  misorder two waiters but cannot break antisymmetry, and antisymmetry is the
  whole no-livelock argument.
* **Do not order gates among themselves in this queue.** They serialise on
  `gate.lock` already, and putting a gate at a queue position is the one way to
  get a run in front of one.
* **Do not "fix" a slow instrument by giving the test more time.** `anchor-sole`
  goes green again if its hold is raised from 8 s to 30 s, and the defect it was
  reporting — `status` taking longer than the run it describes — stays in the
  tree. It was one line (`-ef` for `readlink`).

## Next item, and what closes it

Nothing on this branch. The queue is falsified and self-contained. If anybody
extends it, the closing condition is unchanged: **every property gets an
injection that turns a named case red**, and `CORPUS_LOCK_FALSIFY_DRYRUN=1
./ci/corpus-lock-falsify.sh` (two seconds) must report `patch-ok` for all
nineteen first — four regexes went stale in the single commit that added the
queue, and a patch that has stopped matching is a hole whether or not the box
has twenty minutes spare.
