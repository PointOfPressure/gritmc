# COLD START — branch `corpus-lock-hold-span`

### COLD START — branch `corpus-lock-hold-span`, off `main` (`4674c952`), 2026-08-18 — the in-lock build was the wrong suspect, and the founding evidence for the lock is confounded

Worktree `~/grit/work-hold`, remote dir `~/rustmc-hold`. Extends
`docs/CORPUS-LOCK-HOLD.md` (§§6-8) rather than restarting it.

**The brief's lead does not survive measurement, and that is the headline.**
`ci/t2-gameplay.sh` ran two release builds inside the corpus lock, and with
eight lanes editing Rust and a queue that was six deep at 09:47 this reads like
a large serialised term. It is not.

21 timed builds in a gate-shaped tree, running the harness's own two commands:

| arm | n | mean | note |
|---|---:|---:|---|
| warm no-op | 5 | **0.17 s** | cargo reached, compiled nothing |
| `oracle-runner` changed | 3 | 17.2 s | |
| `rustmc-server` changed (58k lines) | 3 | 32.2 s | |
| `rustmc-worldgen` changed (86k lines) | 3 | 37.6 s | |
| `rustmc-proto` changed (root dep — everything) | 3 | 46.8 s | |
| **cold, no `target/` at all** | 4 | **61.7 s** | 60.8-62.7 |

**`cold` bounds every incremental build above**, because it compiles a strict
superset — that is the argument, not the mean. The workspace has four external
dependencies and the box has 24 threads. So the in-lock build was **2.3% of a
~44 minute hold**.

**And on a real gate it is smaller still, which corrects a figure of my own.**
I first reported an instrumented in-situ build of **3 m 36 s** at load 96 and
called it ~8% of the hold. That is a real measurement and it is not a gate's:
the tree had had its `target/` deleted by the replay and its sources re-synced
immediately before, so cargo rebuilt nearly everything. **A real gate directory
is warm** — `ci/handoff.sh` gives each lane a persistent `RUSTMC_REMOTE_DIR`
whose `target/` survives its previous gate.

Measured on a live merge run instead, by two instruments that share no input:
`ci/corpus-hold-census.sh` timed `rustmc-cov4`'s hold end to end at **46.47
min**, and `ci/corpus-phase-mtimes.sh` put that same run's record work at
**45.25 min** (N=113, reference 22.42, subject 22.83). **Everything else it did
inside the lock — port probe, both release builds, both boots, the comparison,
the teardown — is 1.22 min, 2.6% of the hold**, and the build is a fraction of
that.

Load still matters for the build in absolute terms (cold: 61.7 s at load 3-8,
94.5 s at load 16-40, and the box reaches 96), which is the same class as T0's
`readiness-falsify` being RED at load 11.8 and GREEN at 3.5 — it is just not
the term that governs the hold.

**Moved anyway, and nobody should expect the queue to drain faster for it.**
The throughput return is at most 1.22 min of 46.5 and realistically far less.
The brief's premise — "every queued run is serialised behind every other run's
compilation, and with eight lanes editing Rust concurrently that is not a small
term" — is, measured, a small term. The move stands on its other reasons: it is free; it
empties the acquire→port-bind gap that `ci/t2-gameplay.sh`'s own comment
concedes is a hazard ("something can take the port between the probe and the
bind") — measured at 0.04-0.48 s where a whole build used to sit; and a failing
build now fails without having taken the box. The acquire's three documented
properties (lines 227-249) all survive and are asserted from the file itself.

**`ci/corpus-exclusion-falsify.sh` — 10 cases, ~19 s, in T0.** The property is
two-sided: *while one run is recording, a second run may build, and may not
reach a recording.* Two scratch trees, private lock namespace, the real
`ci/t2-gameplay.sh` end to end against stubs. **Case 7 is the negative
control** — it shadows `corpus_lock_acquire` with a no-op and requires both
runs to record at once, which is what makes cases 2, 3 and 6 known to be able
to fail. Case 6 is interval arithmetic from the stub's own start/stop stamps,
not a count at one instant.

**It has no hold constant.** The first run's recording blocks on a sentinel the
observer removes; the hold is exactly as long as the observation takes, at any
load. Every "hold for N seconds" constant in a lock falsifier on this box has
the same defect and the same fix.

**`ci/hold-phases.sh` — the hold decomposed from inside the run, always on.**
The three instruments in `docs/CORPUS-LOCK-HOLD.md` §1 are two forensic ones
that can only see phases which write a file, and one external sampler that has
to be armed by somebody who guessed right about when a gate would reach stage 6
— which is why it collected nothing on 2026-08-16 while armed and correct. This
one costs ~2 ms across a 43-minute hold, so it runs on every run and every gate
and the N accrues unattended. **The reader decides what is inside the hold from
the timestamps rather than a hardcoded layout**, which is what makes its report
the falsifier for the build's move as well as its measurement.
`ci/hold-phases-falsify.sh`, 12 cases, includes 40 concurrent writers for
append atomicity and both sides of the layout question.

**The reference cache's open row, half closed without the lock.**
`ci/refcache-serve-bench.sh`: store 0.088 s, **serve 0.106 s at N=20** —
0.008% of the 21.3 min of reference recording a hit replaces. The key
re-hashes a 25 MB jar, a 108-file corpus and three tables on every serve by
design, so this was worth measuring rather than assuming. The bench refuses to
report if any lookup missed, because a miss returns before it copies and would
have produced a fast, confident, meaningless number; that guard fired on its
first run. **The direct hit hold is still N=1** — a batched chain is detached
in `~/rustmc-hold`, limited to three holds, and every future run now
self-reports.

**THE LARGEST FINDING IS ONE I DID NOT ACT ON.** `92c47ea3` introduced
`ci/corpus-lock.sh`. It also introduced `pick_port_pair`. Before it, all sixty
trees shared ports 25696/25697 — so the three runs that overlapped on
2026-08-06 were three recorders and **one pair of ports**, and "three recorders
driving one vanilla" explains the recorded evidence better than "concurrent
runs wait out each other's timers", including the one number the accepted
account never explains: **load average 0.02**. Three live corpus runs on a
24-thread box are not 0.02; one idle JVM between commands is exactly 0.02.

Nothing here relaxes the lock and this branch did not touch it. Nobody has run
two corpus recordings concurrently since per-tree ports existed, so there is no
measurement either way and untested is not false. But the hold is ~97%
recording, the reference cache addresses the first half and **nothing addresses
the second**, and the only reason nothing does is a mechanism last observed
before the bug that plausibly caused it was fixed in the same commit.
**97.4% of a real hold is the two recordings** (N=113: 48.2% reference, 49.1%
subject, 2.6% everything else). That is the whole of the opportunity.
`docs/CORPUS-LOCK-HOLD.md` §8 writes down the experiment that would settle it,
why its measurement is recording *agreement* rather than wall time, and why it
must not be run where it can contend with a live gate.
