#!/usr/bin/env bash
# corpus-lock.sh — one corpus run on this box at a time, and the gate goes first.
#
#     . "$ROOT/ci/corpus-lock.sh"          # as a library, from a corpus harness
#     corpus_lock_acquire                  # blocks until it is this run's turn
#
#     ./ci/corpus-lock.sh status           # as a command, for a human
#     ./ci/corpus-lock.sh status --watch
#
#     RUSTMC_CORPUS_LOCK_ROLE=gate ./ci/corpus-lock.sh hold -- CMD...
#                                          # one hold across several harnesses
#
# # What went wrong, and why remembering was never going to fix it
#
# On 2026-08-06 three corpus runs ran at once on the Threadripper — two agents
# and a merge gate. Nothing crashed. What happened instead is worse than a
# crash, because it looks like nothing:
#
#   * a corpus run spends most of its wall clock in `settle_ms` sleeps and in
#     vanilla boots, not on a core. Concurrent runs therefore do **not** share
#     a busy CPU and finish in slightly longer than one run each. They each
#     wait out the others' timers, and throughput fell to roughly **one
#     scenario per ten minutes**;
#   * at a load average of **0.02**. The low load actively misleads. One agent
#     read it as "nothing is contending, so my run must be wedged" and spent
#     the turn hunting a stalled process that did not exist.
#
# Cost: one run killed at 74/86, one at 50/86, one abandoned at 70/86, and a
# gate that took hours. Every one of those ended because a human noticed and
# killed something.
#
# The rule — *a corpus run and a merge gate must never overlap; the gate wins*
# — was written down and was correct. It was enforced by agents remembering it.
#
# # It happened again while this file was being written
#
# Not reconstructed from the 08-06 incident: **measured on 2026-08-07, on
# unlocked code, while the fix for it was being built.** `~/rustmc-chatcmd`'s
# corpus run and the `plugin-hook` merge gate in `~/rustmc-gate` overlapped
# from 02:02 to 02:35. Over 02:02-02:13 both were watched from outside:
#
#   * each `oracle-runner record` accumulated **1-2 CPU-seconds in 17 minutes**;
#   * each vanilla log sat at **2 client joins for 7+ minutes**, no writes;
#   * the **one-minute load average read 0.06, 0.12, 0.23, 0.30** across those
#     samples.
#
# Two independent instruments — kernel CPU time and the reference server's own
# log — agree that almost nothing happened. The third, the load average, said
# the machine was idle, and it was *right*: the machine WAS idle. Both runs
# were asleep in each other's `settle_ms` timers.
#
# **That is why the load average must never be used to decide whether this box
# is busy.** It measures runnable threads, and the pathology here is the exact
# opposite of runnable threads. On 2026-08-06 an agent read 0.02 as "nothing is
# contending, so my run must be wedged" and spent its turn hunting a stalled
# process that did not exist. The number was accurate and the inference was
# backwards. `./ci/corpus-lock.sh status` answers the question the load average
# cannot: not "is the CPU busy" but "does someone already have the box".
#
# # THE 08-07 PARAGRAPH ABOVE IS WITHDRAWN AS EVIDENCE OF A STALL. 2026-08-18.
#
# Everything above this block is left exactly as it was written, because what
# was believed on the night the lock was built is part of why the lock reads
# the way it does. But the 08-07 half of it does not survive its own primary
# artefacts, and a reader who takes it at face value will conclude something
# the evidence does not support. `docs/CORPUS-LOCK-HOLD.md` §13 is the long
# form; the short form is four points:
#
#   * **it is one sample, not a rate.** `gatelock-logs/live-overlap-evidence.txt`
#     is four lines taken at 02:10:43, recording `cputime=00:00:01` from `ps`
#     at one-second resolution. There is no second sample to difference it
#     against, so "1-2 CPU-seconds in 17 minutes" is a reading, not a
#     measurement;
#   * **none of the three instruments can tell a stalled run from a healthy
#     one.** `ci/corpus-healthy-baseline.sh` reads them off whichever run holds
#     this lock — a run that by construction has the box to itself. On
#     2026-08-18 a healthy holder read `00:00:01` at 6m14s elapsed, sat at two
#     `joined the game` lines with no log writes for 357 s, and was at 14/108.
#     `record` logs its party in ONCE per run and reuses it, so two joins is
#     what a healthy 88-scenario recording shows at minute 1 and minute 40
#     alike. Two joins is a constant, and it was read as a rate;
#   * **the stated mechanism is not possible.** "They each wait out the others'
#     timers" requires a settle window that another process can extend. It is
#     `pump_both(.., Instant::now() + Duration::from_millis(settle_ms), ..)`
#     against `while Instant::now() < until` — a fixed deadline, computed once.
#     Two runs' settle windows elapse concurrently, not end to end;
#   * **the run this file calls stalled finished a clean corpus.** The second
#     line of that evidence file is `~/rustmc-chatcmd`'s recorder. It started
#     01:44:17 on a box checked clear, recorded 88/88 against vanilla on :26694
#     and 88/88 against rustmc on :26695, and finished 02:19:51 with
#     `declared 58/88` and `0 vacuous, 0 inert, 0 error` — 2134 s for 176
#     scenario recordings, 12.1 s each including both boots, against the ~9 s
#     solo figure quoted below. At its finishing instant two other corpus
#     recordings were live on the box, on :26503 and :26666.
#
# WHAT THIS DOES NOT DO, AND IT MATTERS THAT IT DOES NOT.
#
# It does not remove the lock and nothing here is a licence to. The 08-06
# incident is untouched by any of it: that one had all sixty trees sharing
# ports 25696/25697, real counter readings, and a real collapse. And rate is
# not integrity — one overlapped run coming out clean is one run. Whether
# exclusion is required for *correctness* is `ci/corpus-exclusion-experiment.sh`
# and `docs/CORPUS-LOCK-HOLD.md` §12 and §14, and changing this lock is the
# owner's call, not a lane's.
#
# # What a corpus run should cost, which nobody had written down
#
# Solo, under this lock, on the Threadripper, read from the harness's own
# `recorded N/86` counter: **8 scenarios in the first ~70 s of recording, about
# 9 s per scenario.**
#
# That figure is the point of quoting it. Before it, "the corpus is slow today"
# was a feeling; contention could only be argued about. Against ~9 s/scenario,
# the overlapped pair above — 2 scenarios in 11 minutes by the joins proxy — is
# two orders of magnitude off, and anybody can now say so in one command
# instead of a paragraph.
#
# The two figures are NOT from the same instrument and must not be quoted as
# one ratio. The solo number is the harness's own counter; the overlapped one
# is a proxy, because those runs' stdout went to their agents' terminals and
# could not be read. Both proxies for the overlapped pair agree, which is why
# it is stated at all — but a ratio built across two instruments is exactly the
# kind of number this project keeps having to withdraw.
# This repo has now found several times over that a guard which depends on
# somebody remembering is not a guard: the gamerule ids that silently did
# nothing, the `--lib` selector that reached none of a binary crate's tests,
# the worktree that has to be detached by hand. So this file makes the
# overlapping run **impossible** rather than discouraged.
#
# # The mechanism, and why this one
#
# Two `flock(2)` locks, and no protocol invented on top of them.
#
#   * **`run.lock`** is the mutex. It is held for the whole run.
#   * **`gate.lock`** carries priority. A gate holds it exclusively from the
#     moment it wants the corpus until the moment it has it.
#
# A gate acquires `gate.lock`, then `run.lock`, then drops `gate.lock`.
# Anything else does this:
#
#     loop:
#         park on the token of the oldest run queued ahead of us, if any
#         flock -s gate.lock ; flock -u gate.lock     # wait out a declared gate
#         flock -x run.lock                           # wait out the current run
#         if a gate has declared intent:   hand run.lock back and loop
#         if an older run is queued ahead: hand run.lock back and loop
#         else: keep it, run
#
# **The hand-back is the whole of the priority mechanism, and it is there
# because `flock` cannot express priority.** Linux wakes one waiter of its
# choosing; it is not a queue and there is no way to ask for one. So a waiting
# run that wins the race against a waiting gate does not get to keep what it
# won — it drops the mutex before it has spent anything, and goes back to
# waiting. Nothing is lost by that: at the point of the check the run has
# booted no server, recorded nothing and measured nothing. It has the lock and
# a `$SECONDS` reading.
#
# This is the brief's second option — the gate signals a waiting run to hold —
# rather than the first, a strict queue the gate jumps. A strict queue would
# have to be built in userspace on top of `flock` (tickets, ordering, a
# rule for what happens when a ticket-holder dies), and every line of it would
# be a lock protocol this file invented. The hand-back needs no queue at all
# and its correctness argument fits in a paragraph:
#
# **AN ORDER WAS BUILT ON TOP AFTER ALL, ON 2026-08-12, AND THE OBJECTION IN
# THAT PARAGRAPH IS WHAT SHAPED IT** — see "Fairness among runs" below. It is
# not a ticket queue: there is no ticket number, no sweeper, and no rule for a
# dead ticket-holder, because a queued process's place is an `flock` it holds
# rather than a number it was given, and the kernel drops it when the process
# does. That objection was right about tickets and it is the reason the order
# is expressed the same way the lock itself is. The four bullets below still
# hold verbatim; the queue adds one more ordering constraint and it is stated
# there.
#
#   * **no deadlock.** The order is always `gate.lock` then `run.lock`. The one
#     place that takes them the other way round — the post-acquire check — is
#     `-n`, non-blocking, so it cannot wait on anything;
#   * **no livelock.** A run that hands `run.lock` back immediately blocks on
#     `flock -s gate.lock`, which the gate still holds exclusively. It cannot
#     re-take the mutex. The gate is unblocked, takes it, and only then
#     releases `gate.lock`;
#   * **a gate is never overtaken.** Any run that takes the mutex after the
#     gate declared intent sees the intent and hands it back;
#   * **a running run is never preempted**, and that is deliberate. Fifty
#     minutes of corpus is not worth throwing away to save a gate twenty of
#     them. A gate waits out at most the one run already in progress.
#
# # Staleness — there is none, and that is the point of using flock
#
# Three runs were killed mid-flight on 2026-08-06, so "a dead holder must not
# wedge the box" is not hypothetical. A lockfile protocol would need a liveness
# check here, and every liveness check invented in this repo has eventually
# been found to be measuring itself.
#
# `flock` needs none. **The lock is a property of an open file descriptor, and
# a file descriptor cannot outlive the process holding it.** `kill -9` closes
# it; the kernel drops the lock. That is liveness based on the holder's process
# actually existing, checked by the kernel on every exit path there is,
# including the ones no script gets to run code on. No timeout, no heuristic,
# nothing to tune, and nothing that can be wrong.
#
# **Inheritance is the one thing that can break that, and it is not
# hypothetical — the falsification sweep found it in this file's first draft,
# three cases red.** An fd survives `fork`, so a harness's children hold the
# lock too, and a corpus harness's children are a vanilla JVM, a
# `rustmc-server` and a twenty-minute `oracle-runner`. Those are exactly the
# processes that outlive a `kill -9` of the harness. There is an orphaned
# `java -jar server.jar` on this box that has outlived its run by fifteen
# hours; a Minecraft server does not exit on its own. Under naive inheritance
# that one stray would have held the box-wide corpus lock for fifteen hours —
# the wedge requirement 3 exists to forbid, reintroduced by the very mechanism
# meant to prevent it. Measured, not reasoned about: the first draft made the
# next acquirer wait **120 s on a corpse**, and that was only because the test
# payload was 120 s long.
#
# It is tempting to call that correct — "a live server is still a reason not
# to start a second run" — and the first draft did. It is wrong twice over. An
# idle orphaned server is nearly free, and what collapsed throughput was
# concurrent *drivers* sitting in `settle_ms`, not idle JVMs. And the debris of
# a killed run is not a run: nobody will read its recording, its harness will
# never compare it, its result is already lost. Holding a box-wide lock on
# behalf of it buys nothing.
#
# **So the lock follows the harness process and nothing else, and the kernel
# enforces that too.** After acquiring, the harness forks a childless *anchor*
#
#     setpriv --pdeathsig KILL bash -c 'exec -a rustmc-corpus-lock-anchor sleep infinity'
#
# which inherits the lock fds, and then closes its own copies. From that
# instant the anchor is the only process in the world holding the lock, and
# every child the harness spawns afterwards — JVM, server, runner — is forked
# from a process that no longer has the fd, so none of them can inherit it.
# `--pdeathsig KILL` makes the kernel kill the anchor the moment the harness
# dies, by any means including `kill -9`.
#
# The invariant is one sentence: **exactly one process holds this lock, and the
# kernel kills it when the run does.** That is worth more than the twenty lines
# it costs, because the alternative — "close the fds at every spawn site" —
# is a rule somebody has to remember at each new site, which is the exact class
# of guard this whole file exists to replace. Verified on the box rather than
# assumed: with the anchor up, the only holder is the anchor; after `kill -9`
# of the harness, the lock is free and a child forked in between never had it.
#
# `corpus_lock_assert_anchored` then checks the invariant at runtime, because
# a lock that has quietly gone free is indistinguishable from one that is held
# right up until two runs collide.
#
# `status` shows the same thing from outside: the holder file says who
# *claimed* the lock, and the `/proc` scan says which live PIDs are actually
# holding the fd. Those two disagreeing is the only stale state this design
# has, it is always visible, and clearing it is killing a process a human can
# see is there.
#
# # Fairness among runs — added 2026-08-12, and the measurement that forced it
#
# The paragraph above says `flock` cannot express priority and that an order
# would have to be built on top. For gates that was answered by the hand-back
# and no order was needed. For runs against each other it was left undone, and
# on 2026-08-11 at 20:05 this is what the box looked like:
#
#     holder:  role=run   tree=~/rustmc-stackdec  since 19:42
#     waiters/ 2454442    role=run   tree=~/rustmc-sound   since 18:07  <- 1h58m
#              2766884    role=gate  tree=~/rustmc-stack   since 20:01
#
# `gameplay-sound-coverage` had been queued for one hour fifty-eight minutes
# and had lost the box to at least three runs that arrived after it. It is the
# branch that cannot be gated until its subset-stability ensemble runs, so it
# was not merely slow, it was held out of every merge chain by the wait.
#
# **The waiter files already carried everything an order needs — `since`,
# `role`, `pid`, `pidstart` — and nothing read any of it.**
#
# ## Where the overtaking actually happened
#
# Not in the kernel's choice of which blocked waiter to wake. In the fast path.
# A run that finds the mutex momentarily free takes it with `flock -x -n`,
# succeeds without ever blocking, and therefore never joins the queue and never
# learns there is one. Every time the box came free — after a gate, after a
# run — whoever happened to be starting up in that instant beat everybody who
# had been queued for hours, because `-n` does not queue. `flock` releases by
# waking waiters to re-contend, not by handing the lock to the head of a queue,
# so an arrival racing a wakeup is a fair race, and a fair race repeated every
# fifty minutes is a starvation.
#
# ## The mechanism: a token that IS the wait
#
# Each queued process holds an exclusive `flock` on **its own waiters/ record**,
# for exactly as long as it is queued. That token is the whole protocol:
#
#   * before contending, a run finds the oldest run queued ahead of it and
#     **blocks on that record's token**. It does not touch the mutex at all. So
#     the mutex is contended by one run at a time — the head of the queue — and
#     the kernel's arbitrary choice among waiters stops deciding anything;
#   * a run that takes the mutex anyway (it was the head when it started
#     blocking, and an older run has since queued) hands it straight back, the
#     same way it hands it back to a gate, and parks. It has spent nothing;
#   * the fast path asks whether anybody is queued before taking the mutex, and
#     again after. This is the line the 20:01 arrival went through.
#
# **This adds no userspace liveness whatsoever, and that is the point of using
# the token rather than a ticket number.** A ticket queue would need to answer
# "what if the ticket-holder died", and every liveness check invented in this
# repo has eventually been found to be measuring itself. The token is an
# `flock` on an open file descriptor, exactly like the main lock, so it obeys
# the same sentence: **a file descriptor cannot outlive the process holding
# it.** A queued run that is `kill -9`ed drops its token in the same instant,
# and everything parked behind it is woken **by the kernel**, with no timeout,
# no sweeper and nothing to tune. Measured, not reasoned about: 306 ms after a
# parked process started waiting, which is when the owner was killed.
#
# ## An entry whose process died — three independent answers
#
# Asked because a queue that a corpse can block is worse than no queue.
#
#  1. **The token is already free.** The kernel released it when the process
#     died, so parking behind a dead waiter returns immediately. This is the
#     load-bearing one and it needs no code.
#  2. **`pid` + `pidstart` liveness**, exactly as for the holder record: a dead
#     entry is skipped for ordering and its file removed by whoever notices.
#     `pidstart` is why a recycled PID cannot impersonate one — it is the
#     reason that field was recorded in the first place, and this is the first
#     thing to read it.
#  3. **The kernel is asked directly** whether a candidate still holds its own
#     token (`_cl_record_is_queued`), before anybody parks behind it. A record
#     that is live by every other measure but whose owner has stopped queueing
#     is skipped rather than waited on.
#
# Nothing here is a timeout on a file's age. The one thing that would
# reintroduce staleness — a record without a token, which any tree still
# running the pre-2026-08-12 library writes — is excluded by the `fairv` field:
# an untokened record is not queued behind, because parking on it would return
# instantly and spin. Trees on this box do not all update at once, so that is a
# rollout requirement and not a nicety.
#
# ## Why it cannot livelock
#
# Two waiters yielding to each other forever is the obvious way to get this
# wrong, and the guard against it is structural rather than careful:
#
#   * the order is **`seq` then `pid`, both read from files, both fixed at
#     registration**. It is therefore a strict total order, and — the part that
#     matters — it is antisymmetric and stable: A and B compare the same way
#     every time either of them asks, forever, whatever the clock does
#     afterwards. A "who has waited longer" comparison would not have that
#     property;
#   * so any non-empty set of queued runs has exactly **one minimum**, and that
#     one yields to nobody. Somebody can always make progress;
#   * a yield **blocks** on a token rather than looping, so a run that is not
#     making progress is asleep in the kernel rather than spinning. There is no
#     retry cadence anywhere in this file and nothing to tune;
#   * the wait-for graph follows that same total order — a run only ever parks
#     behind a strictly older one, and gates park behind nobody — so it is a
#     DAG and cannot contain a cycle;
#   * **run.lock is released before parking, never held across it.** Holding it
#     would deadlock immediately against the older run we are yielding to,
#     which is blocked on run.lock. That single line is the only ordering
#     constraint the queue adds, and `ci/corpus-lock-falsify.sh` breaks it
#     deliberately to prove the suite notices.
#
# `seq` is nanoseconds because `since` is seconds and a burst of arrivals
# inside one second would tie; a tie broken only by PID is not FIFO, it is
# arbitrary. `since` keeps its seconds and its meaning for the humans reading
# `status`.
#
# ## What is NOT claimed
#
# **FIFO among processes that are registered as queued.** A run arriving at a
# genuinely idle lock with nobody queued takes it at once — that is the
# uncontended case, not an overtaking. And the fast path's two queue checks
# bracket a `flock -x -n`, so a run arriving in the microseconds between an
# older run registering and this run's second check can still take the mutex
# once — and then hands it straight back at the post-acquire check, having
# spent nothing. What is closed is the case that was actually costing hours:
# arrivals winning the box repeatedly, for as long as they liked, against a
# queue they never looked at.
#
# **Gate priority is untouched and stays absolute.** Gates consult no queue and
# appear at no position in it; a gate arriving at any instant still goes next,
# and two gates still serialise against each other on gate.lock. A run parked
# behind an older run is holding nothing, so it cannot delay a gate either.
#
# # Observability
#
# A silent wait is indistinguishable from a hang, and this project has already
# lost 67 minutes to a hang that looked like work. So a wait is never silent:
# entering one prints who holds the lock and since when, and a heartbeat prints
# how long it has been going every `CORPUS_LOCK_HEARTBEAT` seconds. `status`
# answers the same question from outside, for anybody who did not start the run.
#
# # Scope
#
# The acquire sits inside the harnesses, not inside their callers, and there is
# exactly one acquire site per harness. That is what makes it airtight: there
# is no route to a corpus run that does not pass through it, so the gate and an
# agent are covered by the same line and neither has to remember anything.
# `ci/t2-gameplay.sh`, `ci/t2-gameplay-selfcheck.sh` and
# `ci/t2-gameplay-subset.sh` all boot a vanilla and drive the corpus at it, so
# all three take it. Nothing else does: `ci/gen-vanilla-region.sh` and the
# benches are long and heavy too, but they are a different resource and
# serialising them behind the corpus would cost throughput for no exclusion
# anybody has asked for.

# --- where the lock lives ---------------------------------------------------
#
# A well-known path, per-user, on whichever box the harness is running on —
# which is the Threadripper, because nothing compiles or runs on the laptop.
# It is deliberately NOT under any tree: sixty trees share this box and the
# whole point is that they share one lock. The override exists so the
# falsification sweep can exercise the protocol in a private namespace without
# queueing behind, or wedging, real work.
CORPUS_LOCK_DIR="${RUSTMC_CORPUS_LOCK_DIR:-$HOME/.rustmc-corpus-lock}"
CORPUS_LOCK_HEARTBEAT="${CORPUS_LOCK_HEARTBEAT:-60}"

# fds 200/201 rather than 9/10: low numbers get reused by ordinary redirection
# in a harness that did not know this file existed, and a corpus harness is
# exactly the kind of script that grows a `3<>fifo` later.
_CL_RUN_FD=200
_CL_GATE_FD=201
# Our own waiters/ record, held open and flocked exclusively for exactly as long
# as we are queued. That flock is the queue token; see "Fairness among runs".
_CL_WAIT_FD=202
# Short-lived, inside forked helpers only: somebody else's record, opened to
# park on or to ask the kernel whether they are still queued.
_CL_PROBE_FD=203

# --- the pool, 2026-09-24 ----------------------------------------------------
#
# **`RUSTMC_CORPUS_LOCK_POOL` corpus runs may hold the box at once; the default
# is 3, and a gate holds all of them** (see `_cl_gate_take_all`). Until 2026-09-24 this was a mutex, justified by the 2026-08-06/07
# overlaps above. Those were re-measured on branch `corpus-parallel`, and the
# serialising resource they describe is not the box: it is ONE vanilla driven by
# several recorders (one hardcoded port pair for every tree, and trees whose
# `oracle-work` was a symlink into another checkout). Reconstructed on purpose,
# two recorders against one vanilla reproduce the collapse; two runs on their
# own derived ports and their own work directories do not interact
# (`ci/corpus-shard-equivalence.sh`, `docs/FINDINGS.md` 2026-09-24). What still
# has to be exclusive is therefore per-tree, not per-box, and it is below:
#
#   * **slots.** Slot 0 is `run.lock`, exactly the file a pool of one uses, so
#     `RUSTMC_CORPUS_LOCK_POOL=1` is the old mutex byte for byte and an older
#     library on the box contends for the same file it always did. Slot k > 0
#     is `run.lock.k`. A run holds exactly one slot, through the anchor, on
#     `_CL_RUN_FD`, and every hand-back below releases that slot;
#   * **the queue and the gate are unchanged.** Only the head of the queue
#     contends, a declared gate still makes every run that took a slot hand it
#     back, and a gate still goes before every queued run. What changes is that
#     the head of the queue waits for ANY slot (`_cl_wait_any_slot`) instead of
#     slot 0;
#   * **one run per tree.** Two runs in one tree share `oracle-work/gameplay`
#     and would overwrite each other's recordings mid-run — the 2026-08-03
#     failure by another door. With a pool of more than one, the acquire first
#     takes `oracle-work/.corpus-tree.lock` exclusively and hands it to the
#     anchor with the slot, so it dies with the run exactly as the slot does.
#
# Bounded, not unbounded, because each run is up to `RUSTMC_CORPUS_SHARDS`
# vanilla JVMs at `-Xmx1G` plus as many subjects: the pool times the shard count
# is the number of server pairs the box carries at once.
# Default 3 since 2026-10-01 (`lock-pool-lanes`): lane runs in separate trees
# do not interact (per-tree ports, per-tree `oracle-work`, the 2026-09-24
# reconstruction above, and `shard-equivalence`'s eight concurrent pairs), and
# a gate now drains every slot, so a pool above one no longer lets anything
# record beside a gate. `=1` is still the old mutex.
CORPUS_LOCK_POOL="${RUSTMC_CORPUS_LOCK_POOL:-3}"
_CL_TREE_FD=204
_CL_SLOT_FD0=220
_CL_SLOT=""        # which slot of the pool we hold (0 = run.lock)
_cl_slot_path() { if [ "$1" -eq 0 ]; then printf '%s\n' "$CORPUS_LOCK_DIR/run.lock"; else printf '%s\n' "$CORPUS_LOCK_DIR/run.lock.$1"; fi; }
_cl_holder_path() { if [ "${1:-0}" -eq 0 ]; then printf '%s\n' "$CORPUS_LOCK_DIR/holder"; else printf '%s\n' "$CORPUS_LOCK_DIR/holder-slot$1"; fi; }

_CL_HELD=""        # set once we own run.lock, so release is idempotent
_CL_WAITER=""      # our entry under waiters/, removed on acquire and on exit
_CL_SEQ=""         # our position in the queue order, fixed at registration
_CL_TICKER=""      # heartbeat pid while waiting
_CL_ANCHOR=""      # the childless process that actually holds the lock
_CL_ANCHOR_START="" # its /proc start time, so a recycled PID cannot impersonate it

# The anchor's argv[0]. Cosmetic — but a thing holding a box-wide lock should
# say what it is in `ps` rather than read as `sleep infinity`, and this file is
# about state being visible.
#
# Overridable for one reason only: `ci/corpus-lock-falsify.sh` deliberately
# breaks the anchor and then has to clean up after itself, and a sweep that
# cleaned up by name would kill the anchor of a REAL corpus run in progress on
# the same box. Giving the sweep its own name makes that impossible.
_CL_ANCHOR_NAME="${CORPUS_LOCK_ANCHOR_NAME:-rustmc-corpus-lock-anchor}"

_cl_now()  { date +%s; }
_cl_iso()  { date -Is; }

# The queue's ordering key. Nanoseconds, because `since` is seconds and a burst
# of arrivals inside one second would all tie — and a tie broken only by PID is
# not FIFO, it is arbitrary. `since` keeps its seconds and its meaning, because
# it is what humans and `status` read; this is a second field beside it.
#
# It is NOT required to be monotonic and the design does not assume it is. The
# order is computed from values that were written once, at registration, and
# never change — so a clock that steps backwards can put two waiters in an
# order nobody intended, but it cannot make the comparison disagree with itself,
# and antisymmetry is the whole of the no-livelock argument.
_cl_seq_now() { date +%s%N; }

# Close every lock fd. For forked helpers: an inherited fd shares the open file
# description, so a child that keeps one keeps the lock it carries — which for
# `_CL_WAIT_FD` means keeping a queue token alive after the process that took it
# has died. That is the one way this design could manufacture staleness.
_cl_close_fds() {
    eval "exec $_CL_RUN_FD>&- $_CL_GATE_FD>&- $_CL_WAIT_FD>&-" 2>/dev/null || true
    eval "exec $_CL_TREE_FD>&-" 2>/dev/null || true
}

# The blocking acquires fork a `flock` that lives for the whole wait and
# inherits every fd we hold. If the harness is SIGKILLed the orphan carries on
# blocking, so it must not be carrying our queue token: it would hold that token
# for as long as it took to acquire — up to a whole corpus run — and everything
# queued behind us would wait on a process that no longer exists. Same reasoning
# as the anchor, in the opposite direction: there, one process must keep the
# lock; here, no process but us may keep the token.
#
# The `eval` is only there because bash cannot take a variable as the target of
# a redirection. The locked fd stays a plain quoted argument so that the text
# `flock -x "$_CL_RUN_FD"` still reads — and still patches — the way it did
# before these wrappers existed.
_cl_wait_run_lock()  { if [ "$CORPUS_LOCK_POOL" -gt 1 ]; then _cl_wait_any_slot; else eval 'flock -x "$_CL_RUN_FD"'  "$_CL_GATE_FD>&- $_CL_WAIT_FD>&- $_CL_TREE_FD>&-"; _CL_SLOT=0; fi; }
_cl_wait_gate_lock() { eval 'flock -s "$_CL_GATE_FD"' "$_CL_RUN_FD>&- $_CL_WAIT_FD>&- $_CL_TREE_FD>&-"; }
_cl_declare_gate_intent() { eval 'flock -x "$_CL_GATE_FD"' "$_CL_RUN_FD>&- $_CL_WAIT_FD>&- $_CL_TREE_FD>&-"; }

# **A gate holds EVERY slot, not one** — added 2026-10-01 (branch
# `lock-pool-lanes`). The pool let a gate take one slot like any run, so with a
# pool above one a merge gate recorded beside lane runs; the measurement it
# exists to produce must not share the box with anything. So a gate drains the
# pool: with its intent declared (runs that take a slot hand it back, and no
# run's fast path enters), it takes slot 0, then 1, ... up to
# `CORPUS_LOCK_POOL_MAX - 1`, blocking on each until the run holding it ends.
# The MAXIMUM, not this process's `CORPUS_LOCK_POOL`: the pool size is an
# environment variable read per process, and a gate started with the default
# must still exclude lanes started with a larger one. A run holds at most one
# slot and never waits while holding one, and two gates serialise on
# `gate.lock`, held for the whole drain, so the fixed order cannot deadlock.
# Every slot goes to the anchor; slot 0 is the one `_CL_RUN_FD` names.
CORPUS_LOCK_POOL_MAX=8
_cl_other_fds() { # fd -> "a>&- b>&- ..." for every lock fd except the one given
    local k out="$_CL_RUN_FD>&- $_CL_GATE_FD>&- $_CL_WAIT_FD>&- $_CL_TREE_FD>&-"
    for k in $(seq 0 $((CORPUS_LOCK_POOL_MAX - 1))); do
        [ "$((_CL_SLOT_FD0 + k))" = "$1" ] || out="$out $((_CL_SLOT_FD0 + k))>&-"
    done
    printf '%s' "$out"
}
_cl_gate_drop_all() {
    local k
    for k in $(seq 0 $((CORPUS_LOCK_POOL_MAX - 1))); do
        eval "exec $((_CL_SLOT_FD0 + k))>&-" 2>/dev/null || true
    done
}
_cl_gate_take_all() { # [-n] — every slot, or (with -n) none and status 1
    local k fd nb="${1:-}"
    for k in $(seq 0 $((CORPUS_LOCK_POOL_MAX - 1))); do
        fd=$((_CL_SLOT_FD0 + k))
        eval "exec $fd>>\"\$(_cl_slot_path $k)\""
        if ! eval "flock -x $nb $fd" "$(_cl_other_fds "$fd")"; then
            _cl_gate_drop_all
            return 1
        fi
    done
    eval "exec $_CL_RUN_FD>&$_CL_SLOT_FD0"
    _CL_SLOT=0
    return 0
}

# The fast path: the first free slot, without blocking. Leaves `_CL_RUN_FD` on
# the slot it took, or back on slot 0 when every slot is busy.
_cl_try_slots() {
    local k
    for k in $(seq 0 $((CORPUS_LOCK_POOL - 1))); do
        eval "exec $_CL_RUN_FD>>\"\$(_cl_slot_path $k)\""
        if flock -x -n "$_CL_RUN_FD"; then _CL_SLOT="$k"; return 0; fi
    done
    eval "exec $_CL_RUN_FD>>\"\$(_cl_slot_path 0)\""
    return 1
}

# The blocking path for a pool: wait for WHICHEVER slot frees first.
#
# `flock` can wait on one file. So each slot gets its own waiter, forked with
# the slot's fd already open in this shell: a lock taken through an inherited
# fd belongs to the open file description, which this shell shares, so the
# waiter that wins leaves the lock HERE when it exits. Each waiter is `exec`ed
# `flock`, so killing it cannot orphan a blocked `flock` that would take a slot
# later. The losers are killed and reaped; if two won in the same instant, every
# winner but one is released by closing its fd, which is the last reference to
# its open file description once its waiter is gone.
_cl_wait_any_slot() {
    local k fd pid won rc wrc
    local -a pids
    while :; do
        pids=(); won=""
        for k in $(seq 0 $((CORPUS_LOCK_POOL - 1))); do
            eval "exec $((_CL_SLOT_FD0 + k))>>\"\$(_cl_slot_path $k)\""
        done
        for k in $(seq 0 $((CORPUS_LOCK_POOL - 1))); do
            fd=$((_CL_SLOT_FD0 + k))
            ( _cl_close_fds; exec flock -x "$fd" ) &
            pids+=("$!")
        done
        # `wrc` is the first finisher's status and nothing may overwrite it:
        # the reaping loop below reuses `rc` for the losers, and reading the
        # winner's status after a loser's 143 is how the first draft of this
        # function spun, handing a free slot back on every iteration.
        pid=""; wrc=0
        wait -n -p pid "${pids[@]}" || wrc=$?
        for k in "${!pids[@]}"; do
            if [ "${pids[$k]}" = "$pid" ]; then [ "$wrc" -eq 0 ] && won="$k"; continue; fi
            kill "${pids[$k]}" 2>/dev/null || true
            rc=0; wait "${pids[$k]}" 2>/dev/null || rc=$?
            [ "$rc" -eq 0 ] && [ -z "$won" ] && won="$k"
        done
        if [ -n "$won" ]; then
            eval "exec $_CL_RUN_FD>&$((_CL_SLOT_FD0 + won))"
        fi
        for k in $(seq 0 $((CORPUS_LOCK_POOL - 1))); do
            eval "exec $((_CL_SLOT_FD0 + k))>&-" 2>/dev/null || true
        done
        if [ -n "$won" ]; then _CL_SLOT="$won"; return 0; fi
    done
}

# One run per tree, when runs can overlap at all. Blocking, and announced,
# because the only thing it can be waiting for is this same tree's other run.
_cl_tree_lock() {
    [ "$CORPUS_LOCK_POOL" -gt 1 ] || return 0
    local d="${ROOT:-$PWD}/oracle-work"
    mkdir -p "$d" 2>/dev/null || return 0
    eval "exec $_CL_TREE_FD>>\"\$d/.corpus-tree.lock\"" 2>/dev/null || return 0
    if ! flock -x -n "$_CL_TREE_FD"; then
        echo "corpus lock: another corpus run is live in THIS tree (${ROOT:-$PWD}); waiting for it —"
        echo "corpus lock:   two runs in one tree share oracle-work/gameplay"
        eval 'flock -x "$_CL_TREE_FD"' "$_CL_RUN_FD>&- $_CL_GATE_FD>&- $_CL_WAIT_FD>&-"
    fi
}

# Has a merge gate declared intent? Non-blocking, and it takes the shared lock
# only to find out, so this can never wait on anything — which is what keeps the
# lock ordering (gate.lock before run.lock) free of deadlock even though this is
# called while run.lock is held.
_cl_gate_declared() {
    if flock -s -n "$_CL_GATE_FD"; then
        flock -u "$_CL_GATE_FD"
        return 1
    fi
    return 0
}

# The start time of a PID, from field 22 of /proc/<pid>/stat, in clock ticks
# since boot. Recorded beside the PID so that "the holder is still alive" can
# never be answered by a *different* process that happens to have inherited the
# number. PID reuse is not exotic on a box that has started this many servers.
_cl_pidstart() {
    local p="$1" s
    [ -r "/proc/$p/stat" ] || return 1
    # comm can contain spaces and brackets, so cut from the last ')'.
    s="$(cat "/proc/$p/stat")" || return 1
    s="${s##*) }"
    printf '%s\n' "$s" | awk '{print $20}'
}

_cl_alive() {
    local p="$1" want="$2" have
    [ -n "$p" ] || return 1
    kill -0 "$p" 2>/dev/null || return 1
    [ -n "$want" ] || return 0
    have="$(_cl_pidstart "$p" 2>/dev/null || true)"
    [ "$have" = "$want" ]
}

_cl_role_check() {
    case "${1:-}" in
        gate|run) return 0 ;;
        *) echo "corpus-lock: RUSTMC_CORPUS_LOCK_ROLE must be 'gate' or 'run', got '${1:-}'" >&2
           return 1 ;;
    esac
}

_cl_setup() {
    case "$CORPUS_LOCK_POOL" in
        [1-8]) ;;
        *) echo "corpus-lock: RUSTMC_CORPUS_LOCK_POOL must be 1..8, got '$CORPUS_LOCK_POOL'" >&2
           return 1 ;;
    esac
    mkdir -p "$CORPUS_LOCK_DIR/waiters" || return 1
    # `>>` never truncates. Truncating the lock file would be harmless to the
    # lock itself — the lock is on the inode, not the bytes — but the files are
    # also where a human looks, and an open-for-write that empties them under a
    # live holder is a needless way to lose the only record of who that is.
    eval "exec $_CL_RUN_FD>>\"\$CORPUS_LOCK_DIR/run.lock\""
    eval "exec $_CL_GATE_FD>>\"\$CORPUS_LOCK_DIR/gate.lock\""
}

_cl_describe_holder() {
    local k n=0 f
    for k in $(seq 0 $((CORPUS_LOCK_POOL - 1))); do
        f="$(_cl_holder_path "$k")"
        [ -s "$f" ] || continue
        [ "$n" -eq 0 ] || printf '             and '
        [ "$CORPUS_LOCK_POOL" -eq 1 ] || printf 'slot %s: ' "$k"
        _cl_describe_one_holder "$f"
        n=$((n + 1))
    done
    [ "$n" -gt 0 ] || echo "nobody (no holder record)"
    return 0
}

_cl_describe_one_holder() {
    local f="$1" role pid tree host since ps age anchor
    [ -s "$f" ] || { echo "nobody (no holder record)"; return 0; }
    role="$(sed -n 's/^role=//p'  "$f")"
    pid="$(sed  -n 's/^pid=//p'   "$f")"
    tree="$(sed -n 's/^tree=//p'  "$f")"
    host="$(sed -n 's/^host=//p'  "$f")"
    since="$(sed -n 's/^since=//p' "$f")"
    ps="$(sed   -n 's/^pidstart=//p' "$f")"
    anchor="$(sed -n 's/^anchor=//p' "$f")"
    age=""
    if [ -n "$since" ]; then age=" ($(( ($(_cl_now) - since) / 60 ))m ago)"; fi
    if _cl_alive "$pid" "$ps"; then
        printf '%s pid %s (anchor %s) on %s in %s since %s%s\n' \
            "${role:-?}" "${pid:-?}" "${anchor:-?}" "${host:-?}" "${tree:-?}" \
            "$(date -d "@${since:-0}" -Is 2>/dev/null || echo '?')" "$age"
    else
        printf '%s pid %s in %s — THAT PROCESS IS GONE (record is stale; the lock itself is held by whatever still has the fd, see below)\n' \
            "${role:-?}" "${pid:-?}" "${tree:-?}"
    fi
}

# Which live PIDs actually hold the lock file open. This is the kernel's answer
# rather than the record's, and it is the one that matters: the record says who
# claimed the lock, this says who is still there. `fuser`/`lsof` are not
# guaranteed present on the box, and scanning /proc needs neither.
#
# **`-ef`, not `readlink`, and it is a 110x difference rather than a style
# preference.** `[ "$(readlink "$link")" = "$target" ]` forks a process per fd
# per process on the box: 12.2 s here at 627 processes, measured against 0.111 s
# for the same answer. That is not merely slow, it made this function wrong to
# call — `status` gathered its sections over fourteen seconds, so it could report
# a holder, then an empty fd list, then an empty waiter list, for a run that was
# alive throughout and had simply ended midway down its own output.
# `ci/corpus-lock-falsify.sh`'s `anchor-sole` case failed on exactly that: it
# holds the lock for 8 s and asked an instrument that takes longer than that.
#
# `-ef` is a shell builtin test comparing device and inode, so it needs no fork,
# and comparing inodes is also more correct than comparing the link text: the
# lock lives on the inode, and `/proc/<pid>/fd/N` for an unlinked file reads as
# `path (deleted)` and would never string-match.
_cl_fd_holders() {
    local target="$1" p link out=""
    for p in /proc/[0-9]*; do
        for link in "$p"/fd/*; do
            [ -L "$link" ] || continue
            if [ "$link" -ef "$target" ]; then
                out="$out ${p#/proc/}"
                break
            fi
        done
    done
    printf '%s\n' "${out# }"
}

# --- the queue ---------------------------------------------------------------
#
# One waiters/ record per queued process. It has always carried everything an
# order needs — `since`, `role`, `pid`, `pidstart` — and until 2026-08-12
# nothing read any of it. See "Fairness among runs" in the header.

_CL_R_role=""; _CL_R_pid=""; _CL_R_pidstart=""; _CL_R_seq=""
_CL_R_since=""; _CL_R_fairv=""; _CL_R_tree=""

# Parse a record in one pass and no forks. Called per candidate per loop
# iteration, and the old five-`sed` shape would have made the scan cost five
# processes per waiter per wakeup.
_cl_read_record() {
    _CL_R_role=""; _CL_R_pid=""; _CL_R_pidstart=""; _CL_R_seq=""
    _CL_R_since=""; _CL_R_fairv=""; _CL_R_tree=""
    local k v
    [ -r "$1" ] || return 1
    while IFS='=' read -r k v; do
        case "$k" in
            role)     _CL_R_role="$v" ;;
            pid)      _CL_R_pid="$v" ;;
            pidstart) _CL_R_pidstart="$v" ;;
            seq)      _CL_R_seq="$v" ;;
            since)    _CL_R_since="$v" ;;
            fairv)    _CL_R_fairv="$v" ;;
            tree)     _CL_R_tree="$v" ;;
        esac
    done < "$1"
    [ -n "$_CL_R_pid" ]
}

# A strict total order over queued processes: sequence first, PID as tie-break.
# Strictness and totality are not decoration — they are the no-livelock proof.
# Any non-empty set of queued runs has exactly one minimum, that one yields to
# nobody, so somebody can always make progress. Both operands are read from
# files and were fixed at registration, so the relation cannot change under a
# moving clock the way a "how long have I waited" comparison would.
_cl_is_older() { # <seqA> <pidA> <seqB> <pidB>  -> 0 if A is strictly ahead of B
    local sa="$1" pa="$2" sb="$3" pb="$4"
    [ -n "$sa" ] && [ -n "$sb" ] || return 1
    if [ "$sa" -lt "$sb" ] 2>/dev/null; then return 0; fi
    if [ "$sa" -gt "$sb" ] 2>/dev/null; then return 1; fi
    [ "$pa" -lt "$pb" ] 2>/dev/null
}

# Is somebody still holding this record's queue token? The kernel's answer, not
# the record's: if we can take the token ourselves then nobody has it. This is
# the second, independent answer to "what about a waiter whose process died" —
# the flock goes when the process does, by the same mechanism that makes the
# main lock free of staleness, so a dead waiter's token is already free here
# even if its record file and its PID check both lied.
_cl_record_is_queued() {
    (
        _cl_close_fds
        eval "exec $_CL_PROBE_FD<\"\$1\"" 2>/dev/null || exit 1
        if flock -x -n "$_CL_PROBE_FD"; then exit 1; fi
        exit 0
    )
}

# Wait until the given record's owner has stopped queueing — by acquiring, by
# exiting, or by dying. Blocking on its token rather than polling is what makes
# yielding cost nothing and what makes it impossible to spin: the kernel wakes
# us, and it wakes us on process death too.
#
# CALLED HOLDING NOTHING. Parking while holding run.lock would deadlock against
# the very process we are yielding to, which is blocked on run.lock.
_cl_park_behind() {
    (
        _cl_close_fds
        eval "exec $_CL_PROBE_FD<\"\$1\"" 2>/dev/null || exit 0
        flock -s "$_CL_PROBE_FD" 2>/dev/null || true
    )
}

# The oldest run queued ahead of us, or nothing. Prints a record path.
_cl_older_run_ahead() { # <myseq> <mypid>
    local myseq="$1" mypid="$2" f best="" bseq="" bpid=""
    for f in "$CORPUS_LOCK_DIR"/waiters/*; do
        [ -f "$f" ] || continue
        _cl_read_record "$f" || continue
        [ "$_CL_R_pid" != "$mypid" ] || continue
        # Gates are not in this queue at any position. Their priority is the
        # gate.lock hand-back and it is absolute; ordering them here could only
        # ever put a run in front of one.
        [ "$_CL_R_role" = run ] || continue
        # Only a record written by a library that takes a queue token can be
        # queued behind — parking on one that has none returns instantly and
        # would spin. Trees on this box do not all update at once.
        [ -n "$_CL_R_fairv" ] || continue
        if ! _cl_alive "$_CL_R_pid" "$_CL_R_pidstart"; then rm -f "$f"; continue; fi
        _cl_is_older "$_CL_R_seq" "$_CL_R_pid" "$myseq" "$mypid" || continue
        _cl_record_is_queued "$f" || continue
        if [ -z "$best" ] || _cl_is_older "$_CL_R_seq" "$_CL_R_pid" "$bseq" "$bpid"; then
            best="$f"; bseq="$_CL_R_seq"; bpid="$_CL_R_pid"
        fi
    done
    printf '%s' "$best"
}

# Is anybody at all queued? Asked on the uncontended fast path, where taking the
# lock without asking is exactly how a run that arrived at 20:01 got in front of
# one that had been queued since 18:07. Costs a readdir of an empty directory in
# the common case.
_cl_any_live_waiter() {
    local f
    for f in "$CORPUS_LOCK_DIR"/waiters/*; do
        [ -f "$f" ] || continue
        _cl_read_record "$f" || continue
        [ "$_CL_R_pid" != "$$" ] || continue
        if _cl_alive "$_CL_R_pid" "$_CL_R_pidstart"; then return 0; fi
        rm -f "$f"
    done
    return 1
}

# Take the queue token, then publish the record. In that order, and by rename,
# so that a record is never visible without its token: a scanner that saw one
# would find the token free, conclude we had stopped queueing, and go round
# again — which is a spin rather than a wait. `mv` within a directory is a
# rename, which preserves the inode the flock is on.
_cl_register_waiter() {
    local role="$1" tmp="$CORPUS_LOCK_DIR/waiters/.tmp.$$" f p
    _CL_WAITER=""
    _CL_SEQ="$(_cl_seq_now)"
    # Sweep temporaries left by a process that died between create and rename.
    # Dot-prefixed so the waiters/* glob never sees a half-written record.
    for f in "$CORPUS_LOCK_DIR"/waiters/.tmp.*; do
        [ -f "$f" ] || continue
        p="${f##*/.tmp.}"
        case "$p" in ''|*[!0-9]*) rm -f "$f"; continue ;; esac
        kill -0 "$p" 2>/dev/null || rm -f "$f"
    done
    rm -f "$tmp"
    if ! eval "exec $_CL_WAIT_FD>>\"\$tmp\"" 2>/dev/null; then return 0; fi
    if ! flock -x -n "$_CL_WAIT_FD"; then
        eval "exec $_CL_WAIT_FD>&-" 2>/dev/null || true
        rm -f "$tmp"; return 0
    fi
    {
        printf 'role=%s\n' "$role"
        printf 'pid=%s\n'  "$$"
        printf 'pidstart=%s\n' "$(_cl_pidstart $$ || echo '')"
        printf 'tree=%s\n' "${ROOT:-$PWD}"
        printf 'host=%s\n' "$(hostname)"
        printf 'since=%s\n' "$(_cl_now)"
        printf 'seq=%s\n' "$_CL_SEQ"
        printf 'fairv=1\n'
    } > "$tmp" 2>/dev/null || true
    if mv -f "$tmp" "$CORPUS_LOCK_DIR/waiters/$$" 2>/dev/null; then
        _CL_WAITER="$CORPUS_LOCK_DIR/waiters/$$"
    else
        rm -f "$tmp"
        eval "exec $_CL_WAIT_FD>&-" 2>/dev/null || true
    fi
}

# Remove the record BEFORE dropping the token. A process parked on us wakes only
# when the token goes, and by then there is nothing left for it to re-read. The
# other order leaves a window in which a woken parker finds a record that is
# still present, still owned by a live PID, and no longer locked — and yields
# again to somebody who has stopped queueing.
_cl_unregister_waiter() {
    if [ -n "$_CL_WAITER" ]; then rm -f "$_CL_WAITER"; fi
    _CL_WAITER=""
    eval "exec $_CL_WAIT_FD>&-" 2>/dev/null || true
    _CL_SEQ=""
}

_cl_write_holder() {
    local role="$1" tmp="$CORPUS_LOCK_DIR/holder.$$"
    {
        printf 'role=%s\n' "$role"
        printf 'slot=%s\n' "${_CL_SLOT:-0}"
        printf 'pid=%s\n'  "$$"
        printf 'pidstart=%s\n' "$(_cl_pidstart $$ || echo '')"
        printf 'tree=%s\n' "${ROOT:-$PWD}"
        printf 'host=%s\n' "$(hostname)"
        printf 'commit=%s\n' "$(git -C "${ROOT:-$PWD}" rev-parse --short HEAD 2>/dev/null || echo unknown)"
        printf 'branch=%s\n' "$(git -C "${ROOT:-$PWD}" rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)"
        printf 'anchor=%s\n' "$_CL_ANCHOR"
        printf 'since=%s\n' "$(_cl_now)"
        printf 'sinceiso=%s\n' "$(_cl_iso)"
    } > "$tmp" && mv -f "$tmp" "$(_cl_holder_path "${_CL_SLOT:-0}")" || true
}

# The heartbeat. Started only if we actually have to wait, and started with
# both lock fds CLOSED — a ticker that inherited them would keep the lock alive
# after the run it belongs to had gone, which is the one way this design could
# manufacture the stale holder it does not otherwise have.
_cl_start_ticker() {
    local since="$1"
    [ "$CORPUS_LOCK_HEARTBEAT" -gt 0 ] 2>/dev/null || return 0
    (
        # Close ALL THREE lock fds in the ticker, first thing. It inherited
        # them, and an inherited lock fd in a process that outlives its run is
        # precisely the stale holder this design otherwise cannot produce. The
        # third one is the queue token: a ticker that kept it would keep the
        # whole queue parked behind a run that had already gone.
        eval "exec $_CL_RUN_FD>&- $_CL_GATE_FD>&- $_CL_WAIT_FD>&-" 2>/dev/null || true  # ticker drops every lock fd
        eval "exec $_CL_TREE_FD>&-" 2>/dev/null || true
        while sleep "$CORPUS_LOCK_HEARTBEAT"; do
            printf 'corpus lock: still waiting, %dm so far — held by %s\n' \
                "$(( ($(_cl_now) - since) / 60 ))" "$(_cl_describe_holder)"
        done
    ) &
    _CL_TICKER=$!
    return 0
}

_cl_stop_ticker() {
    if [ -n "$_CL_TICKER" ]; then kill "$_CL_TICKER" 2>/dev/null || true; fi
    _CL_TICKER=""
}

# --- the anchor -------------------------------------------------------------
#
# Hand the lock to a childless process the kernel kills when we die, then drop
# our own copies of the fds. After this returns, exactly one process holds the
# lock and nothing the harness spawns can inherit it.
_cl_anchor_start() {
    setpriv --pdeathsig KILL bash -c "exec -a $_CL_ANCHOR_NAME sleep infinity" &
    _CL_ANCHOR=$!

    # VERIFY BEFORE DROPPING. Closing our fds first and checking afterwards
    # would be the one ordering that can lose the lock silently: if `setpriv`
    # exits — wrong version, exec failure, fork limit — the anchor is gone, and
    # a harness that had already closed its own copies would be running a
    # corpus while holding nothing, believing it held the box. Checking first
    # costs a fifth of a second and cannot lose: we still have the lock, so a
    # refusal here is safe and a success is real.
    #
    # `kill -0` alone is not enough immediately after `fork` — it succeeds
    # before `exec` has had a chance to fail — so this waits, then asks the
    # kernel the question that actually matters: does that process hold this
    # lock file on this fd?
    sleep 0.2
    local want have
    want="$(readlink -f "$(_cl_slot_path "${_CL_SLOT:-0}")" 2>/dev/null || true)"
    have="$(readlink "/proc/$_CL_ANCHOR/fd/$_CL_RUN_FD" 2>/dev/null || true)"
    if ! kill -0 "$_CL_ANCHOR" 2>/dev/null || [ "$have" != "$want" ]; then
        echo >&2
        echo "corpus-lock: the lock anchor did not come up (pid $_CL_ANCHOR)." >&2
        echo "             expected it to hold $want on fd $_CL_RUN_FD, it has '${have:-nothing}'." >&2
        echo "             Refusing rather than running a corpus that only thinks it" >&2
        echo "             holds the box. Nothing has been released; see ci/corpus-lock.sh." >&2
        exit 2
    fi
    # Its start time, so a recycled PID can never answer "is the anchor alive".
    _CL_ANCHOR_START="$(_cl_pidstart "$_CL_ANCHOR" 2>/dev/null || echo '')"

    # Now drop OUR copies. The lock is on the open file description, which the
    # anchor still refers to, so closing these does not release anything — it
    # only removes the fds that every later `fork` would otherwise copy. The
    # tree lock (a pool of more than one only) goes to the anchor the same way.
    eval "exec $_CL_TREE_FD>&-" 2>/dev/null || true
    eval "exec $_CL_RUN_FD>&- $_CL_GATE_FD>&-" 2>/dev/null || true
    _cl_gate_drop_all
}

_cl_anchor_stop() {
    if [ -n "$_CL_ANCHOR" ]; then kill "$_CL_ANCHOR" 2>/dev/null || true; fi
    _CL_ANCHOR=""; _CL_ANCHOR_START=""
}

# `--pdeathsig` is what makes the anchor safe; without it the anchor would
# survive the harness and hold the lock forever, which is worse than not having
# an anchor at all. So it is checked, not assumed, and the check is a refusal.
_cl_require_setpriv() {
    if ! command -v setpriv >/dev/null 2>&1; then
        echo "corpus-lock: setpriv(1) is missing — cannot hold the lock safely." >&2
        echo "             util-linux >= 2.33 provides it. Install it rather than" >&2
        echo "             running the corpus unlocked; see ci/corpus-lock.sh." >&2
        return 1
    fi
    if ! setpriv --help 2>&1 | grep -c -- '--pdeathsig' >/dev/null; then
        echo "corpus-lock: this setpriv has no --pdeathsig, so a killed run would" >&2
        echo "             leave the lock held forever. Refusing. See ci/corpus-lock.sh." >&2
        return 1
    fi
    return 0
}

# --- the invariant, checked rather than trusted -----------------------------
#
# "Exactly one process holds this lock, and the kernel kills it when the run
# does" is the whole design, so it is asserted while the run is live. A lock
# that has quietly gone free — anchor killed by a stray `pkill`, `setpriv`
# behaving differently after an upgrade — is indistinguishable from a held one
# right up until two runs collide, and that collision is the thing this file
# exists to make impossible. Cheap: one `kill -0` and one `/proc` scan.
#
# **One `kill -0`, and deliberately nothing more.** An earlier version also
# scanned `/proc` here for other processes holding the lock file. That was the
# wrong check in the wrong place twice over: it is O(every fd on the box), so
# calling it on any cadence is expensive on a machine running sixty trees; and
# what it found was mostly a concurrent `corpus-lock.sh status`, which is a
# thing we want to happen. The load-bearing question is "is the anchor still
# alive", and that is one signal to one PID. The `/proc` view belongs in
# `status`, where somebody is asking, and that is where it lives.
corpus_lock_assert_anchored() {
    [ -n "$_CL_HELD" ] || return 0
    if ! _cl_alive "$_CL_ANCHOR" "$_CL_ANCHOR_START"; then
        echo >&2
        echo "T2-GAMEPLAY REFUSED — the corpus lock anchor (pid $_CL_ANCHOR) is gone." >&2
        echo "         This run is no longer holding the box-wide corpus lock, so a" >&2
        echo "         second run can start on top of it at any moment. Nothing" >&2
        echo "         measured after this point would be trustworthy." >&2
        exit 4
    fi
    return 0
}

# --- a hold that spans several harnesses ------------------------------------
#
# **`./ci/corpus-lock.sh hold -- CMD...` takes the lock ONCE and keeps it for
# the whole of CMD, and every `corpus_lock_acquire` inside CMD is served from
# that hold instead of queueing again.** Added 2026-09-30 for the merge gate:
# `ci/handoff.sh` ran stage 6 (`t2-gameplay.sh`) and stage 7
# (`t2-persistence.sh`) as two harnesses, each acquiring and releasing, so the
# lock went free between them and a run that had been queued behind the gate
# took it at once — a running run is never preempted, so the gate then waited
# that run out before its persistence stage. Measured by the gate-pipeline
# lane on train-a's gate: 8.2 minutes at stage 7 out of 146.8.
#
# The hold exports `RUSTMC_CORPUS_LOCK_HELD_BY="<anchor pid> <start> <slot>"`.
# An inner acquire honours it only if all three hold, checked, never trusted:
#
#   * the anchor is alive with that start time (a recycled PID is not it);
#   * the kernel says the anchor has that slot's lock file open — the same
#     `/proc/<pid>/fd` question `_cl_anchor_start` asks before it trusts one;
#   * **the anchor's parent is an ancestor of this process.** Without this, a
#     variable leaked into an unrelated shell would let it run beside whatever
#     run owns that anchor, and "inside the hold" is the only thing the
#     variable is allowed to mean.
#
# Anything else is announced and ignored, and the acquire proceeds normally.
# That fallback cannot deadlock on the hold: it is only taken when the named
# anchor does not hold the lock for us. An inner release leaves the anchor
# alone; the hold's own exit releases it, and `--pdeathsig` still kills it if
# the hold is killed, after which every inner `corpus_lock_assert_anchored`
# refuses exactly as it would for its own anchor.
_cl_ancestors() { # print this process's ancestor PIDs, one per line
    local p=$$ pp
    while [ -n "$p" ] && [ "$p" -gt 1 ] 2>/dev/null; do
        pp="$(awk '/^PPid:/{print $2}' "/proc/$p/status" 2>/dev/null)"
        [ -n "$pp" ] || break
        printf '%s\n' "$pp"; p="$pp"
    done
}
_cl_inherited_ok() {
    local p s k want fd ppid anc
    read -r p s k <<<"${RUSTMC_CORPUS_LOCK_HELD_BY:-}"
    [ -n "${p:-}" ] && [ -n "${s:-}" ] && [ -n "${k:-}" ] || return 1
    _cl_alive "$p" "$s" || return 1
    want="$(readlink -f "$(_cl_slot_path "$k")" 2>/dev/null)" || return 1
    for fd in /proc/"$p"/fd/*; do
        [ "$(readlink "$fd" 2>/dev/null)" = "$want" ] || continue
        ppid="$(awk '/^PPid:/{print $2}' "/proc/$p/status" 2>/dev/null)"
        # Captured first, never piped into `grep -q`: under a harness's
        # `pipefail`, `grep -q` exiting on its first match SIGPIPEs the walk
        # and the pipeline reports failure for a true answer. It did, in the
        # first run of `case_hold_spans_harnesses`.
        anc="$(_cl_ancestors)"
        [ -n "$ppid" ] && [[ $'\n'"$anc"$'\n' == *$'\n'"$ppid"$'\n'* ]] && return 0
        return 1
    done
    return 1
}

# --- the acquire ------------------------------------------------------------
#
# Blocks until it is this run's turn, then returns. It does not fail: waiting
# is the default and aborting is not an option this offers. An abort would only
# move the problem into the caller's error handling and throw away the setup
# the caller has already paid for — which for a corpus run is a preflight, a
# table check and a build.
corpus_lock_acquire() {
    local role="${RUSTMC_CORPUS_LOCK_ROLE:-run}" t0 waited ahead announced
    _cl_role_check "$role" || exit 2
    if [ -n "${RUSTMC_CORPUS_LOCK_HELD_BY:-}" ]; then
        if _cl_inherited_ok; then
            read -r _CL_ANCHOR _CL_ANCHOR_START _CL_SLOT <<<"$RUSTMC_CORPUS_LOCK_HELD_BY"
            _CL_HELD=inherited
            echo "corpus lock: held by the enclosing hold, anchor pid $_CL_ANCHOR — not queueing again"
            return 0
        fi
        echo "corpus lock: RUSTMC_CORPUS_LOCK_HELD_BY='$RUSTMC_CORPUS_LOCK_HELD_BY' names no hold this process is inside — ignored, acquiring normally" >&2
    fi
    _cl_require_setpriv || exit 2
    _cl_setup || { echo "corpus-lock: cannot use $CORPUS_LOCK_DIR" >&2; exit 2; }
    _cl_tree_lock

    t0="$(_cl_now)"

    # Fast path. The uncontended case is the common one and should cost one
    # syscall, one readdir of an empty directory, and one line of output.
    if [ "$role" = gate ]; then
        if flock -x -n "$_CL_GATE_FD" && _cl_gate_take_all -n; then
            flock -u "$_CL_GATE_FD"
            _CL_HELD=1
        fi
    else
        # THE QUEUE CHECK IS WHAT MAKES THE FAST PATH FAIR, and its absence is
        # where the measured starvation actually lived: an arrival that finds
        # the mutex momentarily free takes it with `-n` and never blocks, so it
        # never joins the queue and never learns that anybody is in one. The
        # 20:01 arrival went in front of the 18:07 one through this line. Asked
        # BEFORE taking the mutex so a queued run is not even briefly raced,
        # and again after, because a waiter can register in between.
        if ! _cl_gate_declared && ! _cl_any_live_waiter && _cl_try_slots; then
            if _cl_gate_declared || _cl_any_live_waiter; then
                flock -u "$_CL_RUN_FD"
            else
                _CL_HELD=1
            fi
        fi
    fi

    if [ -z "$_CL_HELD" ]; then
        # We are going to wait, so say so, in full, before going quiet.
        _cl_register_waiter "$role"
        echo "corpus lock: WAITING as $role — the box runs $CORPUS_LOCK_POOL corpus run(s) at a time"
        echo "corpus lock:   held by $(_cl_describe_holder)"
        # NOT the raw fd-holder list. It reads as pid soup at exactly the
        # moment somebody is trying to understand why they are waiting: the
        # anchor is in there, but so is this very process (it holds the fd
        # while it blocks) and every short-lived `sed` it just forked. The
        # anchor is the one that matters and `status` explains the rest.
        corpus_lock_waiters | sed 's/^/corpus lock:   also waiting: /'
        echo "corpus lock:   ./ci/corpus-lock.sh status  answers this from outside"
        _cl_start_ticker "$t0"

        if [ "$role" = gate ]; then
            # Declare intent first. Holding gate.lock exclusively is what makes
            # a non-gate waiter hand the mutex back, and it serialises gates
            # against each other. A gate consults no queue: its priority is
            # absolute and stays that way.
            _cl_declare_gate_intent
            _cl_gate_take_all
            flock -u "$_CL_GATE_FD"
        else
            announced=""
            while :; do
                # Wait out any run queued ahead of us, before contending at all.
                # Parking on its token costs nothing and cannot spin; a run that
                # is not at the head of the queue does not touch the mutex, so
                # the mutex is contended by one run at a time and the kernel's
                # arbitrary choice of waiter stops deciding who goes next.
                ahead="$(_cl_older_run_ahead "$_CL_SEQ" "$$")"
                if [ -n "$ahead" ]; then
                    if [ "$ahead" != "$announced" ]; then
                        announced="$ahead"
                        echo "corpus lock: yielding to an older waiting run (pid ${ahead##*/})"
                    fi
                    _cl_park_behind "$ahead"
                    continue
                fi
                announced=""
                _cl_wait_gate_lock        # blocks while a gate has declared intent
                flock -u "$_CL_GATE_FD"
                _cl_wait_run_lock
                if _cl_gate_declared; then
                    # A gate declared intent while we were waiting and we won
                    # the race anyway. Hand it straight back — we have spent
                    # nothing.
                    echo "corpus lock: yielding to a waiting merge gate"
                    flock -u "$_CL_RUN_FD"
                    continue
                fi
                # The same hand-back, one priority level down. An older run can
                # have queued while we were blocked, or have been woken
                # alongside us and lost the kernel's coin toss.
                ahead="$(_cl_older_run_ahead "$_CL_SEQ" "$$")"
                if [ -n "$ahead" ]; then
                    flock -u "$_CL_RUN_FD"   # RELEASE BEFORE PARKING — see header
                    continue
                fi
                break
            done
        fi
        _CL_HELD=1
        _cl_stop_ticker
        # UNREGISTER BEFORE ANCHORING, and the order is load-bearing rather than
        # tidy. This closes the queue-token fd, and `_cl_anchor_start` below
        # forks a process that inherits every fd we still hold. An anchor
        # carrying our token would keep the whole queue parked behind a run that
        # had already started — the anchor outlives the wait by design, which is
        # exactly what makes it the wrong process to be holding a place in a
        # queue. It also releases our place at the right moment: we have the
        # mutex, so we have stopped waiting.
        _cl_unregister_waiter
    fi

    # Hand the lock to the anchor and drop our own fds, BEFORE returning to a
    # harness that is about to fork a JVM. Everything the caller spawns from
    # here on is forked by a process with no lock fd, so nothing can inherit
    # it, and no spawn site has to know this file exists.
    _cl_anchor_start
    _cl_write_holder "$role"
    waited=$(( $(_cl_now) - t0 ))
    if [ "$waited" -ge 2 ]; then
        echo "corpus lock: ACQUIRED as $role after waiting ${waited}s ($((waited / 60))m), anchor pid $_CL_ANCHOR"
    else
        echo "corpus lock: acquired as $role (uncontended), anchor pid $_CL_ANCHOR"
    fi
    # NO `trap ... EXIT` HERE, deliberately. Every harness that takes this lock
    # already has an EXIT trap doing something that matters — stopping vanilla,
    # preserving a crash report, removing the world — and installing one here
    # would silently replace it. That would trade a concurrency bug for a
    # leaked JVM, which is a worse deal than the one we came for.
    #
    # It costs nothing, because **releasing is not what makes the lock safe**:
    # `--pdeathsig KILL` means the kernel kills the anchor when this process
    # dies, on every exit path including `kill -9`. `corpus_lock_release` only
    # makes that happen a few milliseconds sooner and tidies the record, so a
    # harness that forgets to call it is not a bug — and `status` reads the
    # record's liveness from the PID, so it says "THAT PROCESS IS GONE" rather
    # than lying.
    return 0
}

# The kernel already does the important half of this when we exit. This only
# makes it immediate and tidies the record, so `status` says "free" rather than
# naming a process that has gone.
corpus_lock_release() {
    _cl_stop_ticker
    _cl_unregister_waiter
    [ -n "$_CL_HELD" ] || return 0
    if [ "$_CL_HELD" = inherited ]; then
        # The hold owns the anchor and the record; this harness only borrowed.
        _CL_HELD=""; _CL_ANCHOR=""; _CL_ANCHOR_START=""
        return 0
    fi
    _CL_HELD=""
    rm -f "$(_cl_holder_path "${_CL_SLOT:-0}")"
    _cl_anchor_stop
    eval "exec $_CL_RUN_FD>&-" 2>/dev/null || true
    eval "exec $_CL_GATE_FD>&-" 2>/dev/null || true
}

# Printed in SERVICE ORDER — gates first, then runs oldest-first — because that
# is now a fact about what will happen rather than a listing. Before the queue
# existed this was an unordered `ls` and reading it told you nothing about who
# was next; the answer was "whichever one the kernel happens to wake".
corpus_lock_waiters() {
    local f n=0 rows="" rank key now
    now="$(_cl_now)"
    for f in "$CORPUS_LOCK_DIR"/waiters/*; do
        [ -f "$f" ] || continue
        _cl_read_record "$f" || continue
        # A waiter that was killed leaves its file behind. Liveness is the PID,
        # checked against its start time, exactly as for the holder — never a
        # timeout on the file's mtime.
        if ! _cl_alive "$_CL_R_pid" "$_CL_R_pidstart"; then rm -f "$f"; continue; fi
        rank=1; [ "$_CL_R_role" = gate ] && rank=0
        # A record from a tree that predates the queue carries no `seq`; order
        # it by the second it arrived rather than dropping it off the list.
        key="$_CL_R_seq"; [ -n "$key" ] || key="${_CL_R_since:-0}000000000"
        rows="$rows$rank|$key|$_CL_R_pid|$(printf '%s pid %s in %s, waiting %dm' \
            "$_CL_R_role" "$_CL_R_pid" "$_CL_R_tree" \
            "$(( (now - ${_CL_R_since:-0}) / 60 ))")
"
        n=$((n + 1))
    done
    [ "$n" -gt 0 ] || { echo "nobody"; return 0; }
    printf '%s' "$rows" | sort -t'|' -k1,1n -k2,2n -k3,3n |
        awk -F'|' '{ printf "%d. %s\n", NR, $4 }'
}

corpus_lock_status() {
    local nfree=0 k slot holders queue p anchor tag
    mkdir -p "$CORPUS_LOCK_DIR/waiters" 2>/dev/null || true
    echo "corpus lock: $CORPUS_LOCK_DIR"
    queue="$(corpus_lock_waiters)"
    for k in $(seq 0 $((CORPUS_LOCK_POOL - 1))); do
        slot="$(_cl_slot_path "$k")"
        if (
            exec 210>>"$slot"
            flock -x -n 210
        ) 2>/dev/null; then nfree=$((nfree + 1)); fi
    done

    if [ "$nfree" -eq "$CORPUS_LOCK_POOL" ]; then
        echo "  state:   FREE"
    elif [ "$nfree" -gt 0 ]; then
        echo "  state:   HELD — $((CORPUS_LOCK_POOL - nfree)) of $CORPUS_LOCK_POOL slots in use, a run arriving now starts at once"
    else
        echo "  state:   HELD"
    fi
    echo "  record:  $(_cl_describe_holder)"
    for k in $(seq 0 $((CORPUS_LOCK_POOL - 1))); do
        slot="$(_cl_slot_path "$k")"
        [ -e "$slot" ] || continue
        holders="$(_cl_fd_holders "$slot")"
        [ -n "$holders" ] || continue
        echo "  live fd holders of ${slot##*/} (kernel truth, not the record):"
        anchor="$(sed -n 's/^anchor=//p' "$(_cl_holder_path "$k")" 2>/dev/null || true)"
        for p in $holders; do
            tag=""
            if [ "$p" = "$anchor" ]; then tag="  <-- THE ANCHOR: this is what holds the lock"; fi
            printf '    pid %-8s %s%s\n' "$p" \
                "$(tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null | cut -c1-80)" "$tag"
        done
    done
    echo "    (others listed merely have the file open — a waiter blocking on it,"
    echo "     or a concurrent 'status'. Only an anchor holds a slot.)"
    if (
        exec 211>>"$CORPUS_LOCK_DIR/gate.lock"
        flock -s -n 211
    ) 2>/dev/null; then
        echo "  gate intent: none"
    else
        echo "  gate intent: A MERGE GATE IS WAITING — runs will yield to it"
    fi
    echo "  waiting (service order — any gate first, then the oldest run):"
    printf '%s\n' "$queue" | sed 's/^/    /'
}

# --- as a command -----------------------------------------------------------
# Sourced: everything above is defined and nothing runs. Executed: `status`.
# `${BASH_SOURCE[0]}` differing from `$0` is the sourced case.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    set -uo pipefail
    case "${1:-status}" in
        status)
            if [ "${2:-}" = "--watch" ]; then
                while :; do
                    clear 2>/dev/null || true
                    date -Is
                    corpus_lock_status
                    sleep 10
                done
            else
                corpus_lock_status
            fi
            ;;
        waiters) corpus_lock_waiters ;;
        hold)
            # See "a hold that spans several harnesses" above. The role comes
            # from RUSTMC_CORPUS_LOCK_ROLE exactly as for a harness.
            shift; [ "${1:-}" = "--" ] && shift
            [ "$#" -gt 0 ] || { echo "usage: $0 hold -- CMD [ARGS...]" >&2; exit 2; }
            corpus_lock_acquire
            RUSTMC_CORPUS_LOCK_HELD_BY="$_CL_ANCHOR $_CL_ANCHOR_START ${_CL_SLOT:-0}"
            export RUSTMC_CORPUS_LOCK_HELD_BY
            "$@"; _hold_rc=$?
            corpus_lock_release
            exit "$_hold_rc"
            ;;
        *)
            echo "usage: $0 [status [--watch] | waiters | hold -- CMD...]" >&2
            echo "       . $0   then  corpus_lock_acquire   to take it" >&2
            exit 2 ;;
    esac
fi
