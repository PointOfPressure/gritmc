# The corpus lock hold, decomposed

**Measured 2026-08-16 on the Threadripper. Branch `corpus-lock-hold`.**

A merge gate holds `ci/corpus-lock.sh` — the box-wide one — for the whole of
`ci/t2-gameplay.sh`. The `gate-lanes` branch measured the Amdahl ceiling of
running gates in parallel lanes at **1.40x at any number of lanes**, because the
lock is the machine. So the hold is the only thing that moves throughput, and
the question worth asking about it is not how long it is but **what inside it
depends on the branch being gated**.

Answer, and it is close to exactly half:

| phase | rate | at N=106 | share of in-lock record work | depends on the candidate? |
|---|---:|---:|---:|---|
| vanilla reference recording | 12.09 s/scenario | 21.3 min | 49.8% | **no** |
| rustmc subject recording | 12.21 s/scenario | 21.6 min | 50.2% | yes, by construction |
| compare + floor report | — | <0.1 min | <0.3% | yes |

---

## 1. Method

Three instruments, deliberately independent, because a single one measuring the
wrong thing is this project's most common failure and agreement between two
that share an input is not corroboration.

### 1.1 Phase mtimes in a tree that ran a corpus

`ci/t2-gameplay.sh` writes a fixed sequence of files into
`oracle-work/gameplay/`, and each one's mtime is the instant that phase ended:

```
harness-gamerules.txt   the four game rules delivered; the reference recording starts here
vanilla.json            the reference recording ends
rustmc.json             the subject recording ends
report.json             the comparison ends
```

Harvested from every tree on the box holding a full-corpus recording
(`rustmc-stack`, `rustmc-uio`, `rustmc-schema`, `rustmc-dark`, `rustmc-p17` —
n=5, N between 93 and 110), with the scenario count read out of each recording's
own `scenarios` array rather than assumed:

| tree | N | reference | subject | reference share |
|---|---:|---:|---:|---:|
| rustmc-dark | 93 | 18.79 min | 18.92 min | 49.8% |
| rustmc-p17 | 93 | 18.76 min | 19.01 min | 49.7% |
| rustmc-schema | 110 | 22.12 min | 22.43 min | 49.7% |
| rustmc-uio | 107 | 21.54 min | 21.72 min | 49.8% |
| rustmc-stack | 106 | 21.33 min | 21.50 min | 49.8% |

**This instrument's weakness is that mtimes are not timestamps of intent** — a
file touched by anything else would move the boundary. It is used because the
second instrument does not share that weakness and agrees with it.

### 1.2 Twenty-six cold vanilla boots, from the servers' own logs

`ci/t2-gameplay-selfcheck.sh` boots vanilla and records the whole corpus against
it, repeatedly. The recordings from those runs were reported lost in the
2026-08-07 disk failure — **but the vanilla servers' own `logs/latest.log`
survive**, in `oracle-work/gameplay-selfcheck/rN/logs/`, and every line in them
carries a real timestamp written by the reference itself. `oracle joined the
game` to `Stopping the server` is the reference recording, measured by the thing
being measured.

| date | tree | N | runs | reference recording | s/scenario |
|---|---|---:|---:|---|---:|
| 2026-08-09 | rustmc-throw, rustmc-twin | 88 | 8 | 17.53–17.70 min | 12.01 |
| 2026-08-10 | rustmc-cov2 | 93 | 6 | 18.67–18.87 min | 12.14 |
| 2026-08-12 | rustmc-stackdec | 93 | 6 | 18.77–18.93 min | 12.14 |
| 2026-08-13 | rustmc-items | 109 | 6 | 21.88–22.00 min | 12.07 |

**Linear in N with a zero intercept**, and the spread over six runs of one
corpus size is ±0.1 min. The reference recording is not merely stable, it is
almost deterministic in duration.

The two instruments share no input: one reads file mtimes in a subject tree,
the other reads log lines written by a JVM. They agree to 0.2%.

### 1.3 A passive phase sampler, for a live gate

`ci/corpus-hold-sample.sh` samples `./ci/corpus-lock.sh status` and the live
gate log once every 15 s and records when each phase marker first appears. It
reads only, takes nothing, and writes nowhere a gate reads —
`ci/lane-measure.sh` established the pattern.

**It produced no data on 2026-08-16 and the reason is worth recording**: the
gate chain draining `queue7.txt` was failing every branch at T0 or T1 in under a
minute, so no gate reached stage 6 at all while the sampler was armed. The
instrument is committed and correct; it simply had nothing to watch. It is the
right way to confirm the split on a live merge gate when the chain is green
again.

---

## 2. What is branch-invariant, and what is not

**The reference recording is what an unmodified vanilla 26.2 does when the
recorder drives the scenario corpus at it.** Nothing our server produces reaches
it: `record` opens two clients against `127.0.0.1:$VAN_PORT`, performs the
scenario's steps, and writes down what came back. There is no path by which
`crates/rustmc-server` can change a byte of `vanilla.json`.

That is the whole of the claim, and it is smaller than it first looks, because
the reference is **not** a function of the jar and the scenario file alone. Its
real inputs are:

| input | branch-dependent? | measured |
|---|---|---|
| `oracle-work/vanilla-26.2/.../server-26.2.jar` | no — pinned | — |
| `ci/checks/gameplay/*.toml` | **yes** | 19 of 44 live branches touch it |
| `tools/oracle-runner/src/*.rs` (the recorder) | **yes** | counted in the same 19 |
| `ci/t2-gameplay.sh`, `ci/vanilla-server.sh` (substrate) | **yes** | rarely |
| `data/vanilla/{items,block_props,entity_types}.json` | **yes** | see below |
| `GAMEPLAY_SEED` | no in practice | — |
| `crates/rustmc-server` (our server) | **no** | the point |

**The tables are the input the naive argument misses.** `client::Tables::load`
reads exactly three files, and they decide which item id and which block state
the harness sends *to vanilla*. They are generated from the tree's own source,
so they are branch-dependent in principle. Measured across six trees on
2026-08-16: byte-identical in five, and different in `rustmc-items` — the tree
whose branch was about the item registry. That is the correct behaviour of a
content key, observed rather than argued: a miss for the branch that moved the
input, a hit for everyone else.

### How often the reference is genuinely unchanged

Two units, because they answer different questions:

* **per commit on `main` since 2026-08-01**: the reference key is unchanged
  across **86.6%** of 1,101 commit transitions. 233 commits touch `crates/` and
  none of the reference inputs; 32 touch both.
* **per branch, over the 44 live branches with work**: unchanged for **25
  (56.8%)**. The 19 misses are the gameplay, trace and corpus branches, which
  add scenarios by definition.

The first number is the one that governs re-gates — the chain re-gates every
branch each time `main` moves — and the second governs a branch's first gate.
The truth is between them and both are worth more than the 1.40x that lanes can
buy.

---

## 3. Two figures in the originating brief that this measurement refutes

Per `docs/AGENT-BRIEF.md`: trust the tree over any figure in a brief.

* **"the hold is 38.4 min"** — too low for the current corpus, and low by
  construction. It was recovered from waiter heartbeats in the lock logs, and a
  waiting gate's heartbeat measures the time **from its own arrival to the
  release**, not the holder's full hold. Any waiter that arrives after a holder
  starts undercounts it, and none can ever overcount. Directly measured, the
  record work alone is **42.8 min at N=106** (21.3 + 21.5), before the in-lock
  `cargo build` and the vanilla boot. 38.4 min is a good match for a corpus of
  ~93, which is what the box was running on 2026-08-10/12.
* **"a merge gate takes 53.9 min"** — not a typical full gate. Of 86 gate
  invocations in `gate-chain.log`, 64 ended in under 30 min because they died at
  T0 or T1; the 22 that got further ran **55.2 to 177.7 min**, median 81.6.
  53.9 min is below the fastest full gate observed.

Neither correction weakens the item. Both make it larger: the serialised
section is bigger than the brief thought, and half of it is the half that does
not depend on the branch.

---

## 4. `ci/reference-cache.sh`

Record the reference once per (jar, corpus, recorder, projection, substrate,
tables, seed); serve it to every gate whose inputs match.

**Content hashes, never paths.** The 2026-08-03 defect — a gate reporting 12/55
with 11 regressed for a branch that was fine — was `oracle-work` and
`data/vanilla` shared *by path*, which hands you whatever was written last with
nothing able to tell. The difference here is structural, not diligence:

* shared by path → you get the last write, and nothing can tell;
* shared by content hash → you get exactly the artefact whose inputs you named,
  or you get nothing.

No mtimes anywhere in the key (`rsync -a` moves those backwards and has already
fooled cargo here once), no version strings, no branch names.

**The key is not opaque.** Every component is stored beside the entry and
re-derived on every serve; a miss diffs against the nearest stored entry and
**names the component that moved**. The first implementation printed only "no
entry for `<key>`", which is a cache that cannot say which of eight inputs
changed — and, worse, a test that cannot show it missed *because of* the
mutation, which is tier 1 in `ci/corpus-falsifiable.py`'s sense. Its own
falsifier caught that.

### Evidence

Both in `ci/t0.sh`. Neither needs a jar, a server, a recording or the lock.

* **`ci/reference-cache-falsify.sh`** — 22 cases, ~2 s, private `REFCACHE_DIR`.
  Every key component mutated in turn misses *and names itself*; a truncated,
  tampered, or component-mismatched entry is refused rather than served; a
  missing input refuses by name instead of hashing to the empty string; touching
  every file's mtime leaves the key unmoved; the 2026-08-03 shared-tables
  instance is reconstructed and caught; and `Tables::load`'s file set is
  re-derived from its own source so a fourth table cannot silently fall outside
  the key.
* **`ci/reference-cache-integration.sh`** — 6 cases, ~5 s. Drives the **real**
  `ci/t2-gameplay.sh` twice over a scratch tree with a stubbed
  `ci/vanilla-server.sh` and a private lock namespace. `start` **is** reached on
  a miss and is **never** reached on a hit, and the hit run carries on to the
  subject half with the served reference and still puts it through
  `assert_recording`. This is the case the unit falsifier cannot see: a cache
  that answered perfectly into a harness that booted vanilla anyway would pass
  all 22 cases above and save nothing, silently, because the recording would
  still be correct.

### The hazard the cache creates, and its guard

**A cache that serves one recording to N trees destroys the input supply of
`ci/corpus-floor.sh`.** That script's own usage line is
`./ci/corpus-floor.sh ~/rustmc*/oracle-work/gameplay/vanilla.json` — every tree
that hits the cache leaves a byte-identical file at exactly that path.
Byte-identical recordings disagree by zero at every scenario, so enough of them
drive every scenario to `flaps == 0`, which that script reports as *the
reference reproduced it*: **a floor of zero, manufactured by the instrument**,
in the file whose own header says a floor of 0 from one recording would be this
project's favourite kind of defect.

It had never been checked because it had never been possible. Measured
2026-08-16: 55 such recordings exist on this box and all 55 are byte-distinct.

`ci/corpus-floor.sh` now refuses byte-identical recordings passed as separate
runs, before the release build, naming both paths. It refuses rather than
deduplicating: a caller who believes they passed six independent runs and passed
two has a problem with their inputs, and a quietly-reduced N publishes a
resolving power nobody asked for. Asserted by the `floor-dupes` cases — copies
refused, distinct recordings still accepted, because a guard that fired on real
ensembles would be removed by the first person it blocked.
`ci/corpus-floor-falsify.sh` remains 21/21.

### Status: opt-in, default off

`RUSTMC_REFERENCE_CACHE=1` enables it; unset, `ci/t2-gameplay.sh` boots vanilla
exactly as before. Repo policy for new behaviour, and doubly right here, because
the measurement that would retire the flag is a live merge gate run both ways —
which this branch was not permitted to run.

**So the hold reduction is projected from a measured quantity, not observed.**
The projection is that a hit removes 21.3 min of the ~43 min in-lock record
work, taking the hold to ~21.6 min: **1.99x on the serialised section**. What is
measured is the 21.3 min and the fact that no vanilla boots on a hit. What is
not yet measured is a live gate's hold with the flag on. Do not quote a new hold
figure until someone has.

---

## 5. Next, in order

1. **Turn the flag on for one gate and measure the hold both ways.** That is the
   one missing number. `ci/corpus-hold-sample.sh` is committed for exactly this
   and needs the chain to be reaching stage 6.
2. **Re-run `ci/lanes`' Amdahl calculation against the new serial fraction.**
   The 1.40x ceiling was computed against a hold that was 71% of the gate.
   Halving the hold does not compete with lanes, it raises their ceiling — the
   two multiply.
3. **Move `cargo build` outside the lock.** It sits inside it today
   (`ci/t2-gameplay.sh`, after `corpus_lock_acquire`), and a build boots no
   vanilla and needs no exclusivity. Not done here because it changes what the
   lock means and deserves its own measurement rather than being smuggled in
   beside a cache.
4. **Make the cache a reference-stability instrument instead of a threat to
   one.** An entry carries `recorded-at`; a policy of periodically re-recording
   and *comparing against the cached copy* turns each refresh into a free sample
   of vanilla's disagreement with itself — the quantity `ci/corpus-floor.sh`
   exists to measure and the quantity a naive cache silently starves.

---

## 6. The hold is now measured from inside the run

**2026-08-18, branch `corpus-lock-hold-span`.**

§1 used three instruments and §5's first item asked for a fourth thing none of
them could give: the hold of a *live* gate, both ways round the reference
cache. That item did not move for two days, and the reason is a property of the
instruments rather than of anybody's diligence.

* §1.1 and §1.2 are **forensic**. They read artefacts a run happened to leave —
  file mtimes, a JVM's own log — so they can only see a phase that writes a
  file. The acquire wait, the in-lock `cargo build` and whether the reference
  came from the cache leave no artefact at all, and those are exactly the
  phases a *decision* about the lock turns on.
* §1.3, `ci/corpus-hold-sample.sh`, is **live but external**, and it has to be
  armed while a gate is in stage 6. It was armed and it collected nothing,
  because the chain was failing at T0 or T1. **An instrument you have to be
  present for measures the runs you were present for**, which on this box is a
  biased sample of the runs that got far enough to be worth watching.

`ci/hold-phases.sh` is the fourth instrument and it is inside the run.
`ci/t2-gameplay.sh` calls `hold_phase` at each boundary; each call costs one
`date`, one 24-byte read of `/proc/loadavg` and one append — about 2 ms across
a 43-minute hold — so it is always on, for every run and every gate, and the N
accrues with nobody watching. Sixty trees append to one file, which is safe
because every write is a single short `printf >>` to an `O_APPEND` file;
`ci/hold-phases-falsify.sh` case 9 asserts it with 40 concurrent writers rather
than quoting POSIX.

**The reader decides what is inside the hold from the timestamps, never from a
hardcoded layout.** The hold is `acquire-end .. release`; a span belongs to it
if and only if it lies between those two instants. That is not fastidiousness —
the `cargo build` moved out of the lock in this same branch, and a reader with
the old layout compiled in would have gone on charging the build to the hold,
wrong by exactly the amount the move was worth. Cases 6 and 7 of the falsifier
feed the reader the same 60-second span on either side of the acquire and
require it to answer differently.

`./ci/hold-phases.sh report` prints the decomposition, the per-scenario rates,
what fell outside the hold, and — when the log holds both — the hit/miss
comparison. A run whose markers are incomplete is **excluded from the N and
named as excluded**, never averaged in.

---

## 7. `cargo build` inside the lock: measured, moved, and much smaller than it looked

Item 3 of §5 said to move it and did not say what it was worth. It is worth
about a minute, and the case for the move survives that.

### What was measured

`ci/t2-gameplay.sh` ran two release builds between the port pick and the
vanilla boot, so every queued run waited out every other run's compilation.
With eight lanes editing Rust concurrently that reads like a large term. It is
not, and the reason is the workspace: four external dependencies
(`flate2`, `serde`, `serde_json`, `toml`), 179k lines of first-party Rust, and
a 24-thread box.

21 timed builds in `~/rustmc-hold`, a gate-shaped tree seeded exactly as
`ci/handoff.sh` stage 2 seeds one, running the same two commands the harness
runs. **Not on a live gate**, deliberately: a build takes no lock, so measuring
it needed none.

| arm | what changed | n | mean | range |
|---|---|---:|---:|---|
| `noop` | nothing; cargo reached and compiled nothing | 5 | **0.17 s** | 0.15–0.17 |
| `touch-recorder` | `tools/oracle-runner` | 3 | 17.2 s | 16.5–18.1 |
| `touch-server` | `crates/rustmc-server` (58k lines) | 3 | 32.2 s | 31.5–33.2 |
| `touch-worldgen` | `crates/rustmc-worldgen` (86k lines) | 3 | 37.6 s | 35.4–39.1 |
| `touch-proto` | `crates/rustmc-proto` — a root dep, so everything | 3 | 46.8 s | 45.9–48.0 |
| `cold` | no `target/` at all | 4 | **61.7 s** | 60.8–62.7 |

**`cold` bounds every other arm from above**, and that is the argument rather
than the mean: a cold build compiles a strict superset of what any incremental
build compiles, so no rebuild of this workspace can cost more than ~62 s at
this load. Against a ~44-minute hold that is **2.3%**, and the realistic arms
are half of it.

*Caveat, and it is the honest one.* These ran at load average 3–8. This box
routinely sits at **load 50–100** with eight lanes working — 97.15 while this
paragraph was written — and a build there is several times slower. So 62 s is
the ceiling on a quiet box, not the ceiling on a working one. The direction of
that error makes the move *more* worthwhile, not less, which is why the move
does not depend on the figure.

### Why it moved anyway

1. **It is free.** Nothing between the old site and the acquire was a
   prerequisite. `BIN` is not read until the reference recording.
2. **It shortens the probe-to-bind gap.** `pick_port_pair` probes a port free
   and `ci/vanilla-server.sh` binds it later; the file's own comment concedes
   "something can take the port between the probe and the bind". A whole
   `cargo build` used to sit in that gap. It is now nothing —
   `ci/corpus-exclusion-falsify.sh` case 10 measures the gap at 0.06–0.48 s.
3. **A failing build now fails without having taken the box.**

### What it did not break

The acquire's position is documented at `ci/t2-gameplay.sh` lines 227-249 with
three properties, and all three survive: still after the preflight, still
immediately before the ports are picked, still one acquire site in the harness
rather than in its callers. `RUSTMC_CORPUS_LOCK_ROLE=gate` and the hand-back
are untouched — this file never mentioned them and still does not. Cases 8-10
of the falsifier derive all of that from the file, so an edit that moves the
acquire past the port pick, or adds a second acquire site, goes red.

### `ci/corpus-exclusion-falsify.sh` — 10 cases

The move means a second run now does real work on the box while a first one
records, so the property under test is two-sided:

> while one run is recording, a second run **may** build, and **may not** reach
> a recording.

Asserting only the second half would pass for a change that never got the
concurrency it was made for; asserting only the first would pass for a change
that removed the lock. Two scratch trees, a private lock namespace, a stubbed
`ci/vanilla-server.sh` whose `start` is the instrument — reaching `start` *is*
reaching a recording — and the real `ci/t2-gameplay.sh` driven end to end, on
the pattern `ci/reference-cache-integration.sh` established.

**Case 7 is the reason the other nine mean anything.** It shadows
`corpus_lock_acquire` with a stub that returns immediately, changes nothing
else, and requires both runs to record at once. Without it, cases 2, 3 and 6
are greens that cannot go red — and a falsifier for a concurrency property that
cannot fail is precisely the defect this repo catalogues. Case 6 is interval
arithmetic over the whole run rather than a count at one instant, because a
count would miss an overlap that opened and closed between two eyeblinks.

In T0 at `CORPUS_EXCL_HOLD_SECS=6`, about 28 s.

### One cost this change imposes, named rather than left to be discovered

`ci/reference-cache.sh` hashes **the whole of `ci/t2-gameplay.sh`** into the
`harness` component of the reference key. So this branch's edit — moving two
lines and adding comments — invalidated every cached reference on the box, and
the first run afterwards missed with `harness` correctly named as the component
that moved. That is the key behaving as designed and it is also a real cost:
**a comment change in `ci/t2-gameplay.sh` costs the box a full re-record.** The
component is deliberately coarse and narrowing it is a separate decision with
its own falsifier to write; it is recorded here so the next person to touch
this file knows what they are spending.

---

## 8. The lock's founding evidence is confounded with a bug fixed in the same commit

**This section changes nothing and proposes nothing. It records a fact about
the evidence, because the fact is worth more than anything else in this
document and acting on it is not this branch's decision to make.**

`ci/t2-gameplay.sh`'s own comment, and `92c47ea3`'s commit message, give the
lock's justification:

> Three corpus runs overlapped on the Threadripper on 2026-08-06 — two agents
> and a merge gate. [...] a corpus run sits in `settle_ms` waits and vanilla
> boots rather than on a core, so concurrent runs wait out each other's timers
> instead of sharing a busy CPU. Throughput fell to about one scenario per ten
> minutes at load average 0.02.

**`92c47ea3` is also the commit that introduced `pick_port_pair`.** Before it,
this file hardcoded ports 25696 and 25697 *for every one of the sixty trees on
the box* — the file says so, forty lines below the acquire. So on 2026-08-06
the three overlapping runs were not merely concurrent: they were three
recorders and one pair of ports.

That matters because it is a better explanation of the same evidence:

| observation | "they wait out each other's timers" | "three recorders, one vanilla" |
|---|---|---|
| ~1 scenario / 10 min | requires a serialising resource nobody has named | each recorder's `settle_ms` quiet is broken by the other two acting on the same world |
| load average **0.02** | unexplained — three live runs on a 24-thread box are not 0.02 | one idle JVM between commands is exactly 0.02 |
| runs reached 74/86, 50/86, 70/86 | consistent | consistent — they progressed, against a shared server, producing garbage |
| the recordings were garbage | asserted | *derived*: three clients drove one world |

The second column also explains why the first was believed: it was written
while the port defect was still being found, in the same hours, and the fix
went into the same commit as the lock.

**What follows from this, and what does not.**

Does not: that the lock is unnecessary. Nobody has run two corpus recordings
concurrently since per-tree ports existed, so there is no measurement either
way, and `docs/AGENT-BRIEF.md`'s rule is to trust the tree over a figure in a
brief — here there is no figure at all. Two recordings on one box producing
garbage is a claim that has never been tested under the current port scheme,
and "untested" is not "false".

Does: **this is the largest unopened question about the corpus lock, by a wide
margin.** The hold is ~44 minutes and ~97% of it is the two recordings. The
reference cache addresses the first half (§4). Nothing addresses the second
half, and the only reason nothing does is a mechanism that has not been
observed since the bug that plausibly caused it was fixed.

The experiment that would settle it, for whoever takes it:

1. two trees, two derived port pairs, a private `RUSTMC_CORPUS_LOCK_DIR` so the
   real queue is untouched, and `ci/t2-gameplay.sh` in both at once;
2. the measurement is **not** wall time. It is whether each run's `vanilla.json`
   is a reading of *its own* vanilla — which `ci/corpus-floor.sh` can answer,
   because it already refuses byte-identical recordings and already has a floor
   for how much two independent vanilla runs disagree (§4). A concurrent pair
   that disagrees within the floor is a pair that did not interfere;
3. per-scenario rate from `ci/hold-phases.sh` against the same run performed
   alone, which is what the 1-scenario-per-10-minutes claim is about.

It must be run somewhere it cannot perturb a live gate, because CPU contention
with a real holder's recording could inject flaps into somebody's merge — which
is why this branch measured the build instead and left this alone.

---

## 9. The whole non-recording part of a real gate's hold is 1.2 minutes

**2026-08-18, measured on a live merge run, and it corrects §7's own figure in
the direction that matters.**

Two instruments that share no input bracket it:

* `ci/corpus-hold-census.sh` read the lock's holder record every 15 s and timed
  `rustmc-cov4`'s hold end to end: **46.47 min**, 09:06:02 to 09:52:30;
* `ci/corpus-phase-mtimes.sh` read the same run's four phase files: N=113,
  reference **22.42 min**, subject **22.83 min**, compare 0.1 s — **45.25 min**
  of record work.

Everything else that run did inside the lock — the port probe, **the two
release builds**, the vanilla boot, the rustmc boot, the comparison, the
teardown — is the difference: **1.22 minutes, 2.6% of the hold.** The build is
some fraction of that 1.22, not the whole of it.

### This refutes §7's in-situ figure, which was mine and was wrong

§7 reported an instrumented build of **3 m 36 s** at load 96 and called it ~8%
of the hold. That number is real but it is not a gate's number: the tree it was
taken in had had its `target/` deleted by the build replay and its sources
re-synced immediately before, so cargo rebuilt almost everything. **A real gate
directory is warm** — `ci/handoff.sh` gives each lane a persistent
`RUSTMC_REMOTE_DIR` whose `target/` survives from its previous gate — so the
arm that describes it is the warm no-op (0.17 s) or a one-crate-changed rebuild
(17-47 s), not the cold one.

Both figures stand as measurements of what they measured. Only the second is a
measurement of a gate. The correct reading of §7's table is therefore that
`cold` bounds the build above **and is not typical of the population**; the
population is at the fast end, and 1.22 minutes for *everything* non-recording
is the tighter and more honest ceiling.

### What it does to the case for the move

It makes the throughput argument smaller and leaves the other reasons intact.

Moving the build out of the lock returns **at most 1.22 min of 46.5**, and
realistically a fraction of that. It is still worth doing — it costs nothing,
it empties the acquire→port-bind gap (0.04-0.48 s where a build used to sit),
and a failing build no longer holds the box while it fails — but **nobody
should expect the queue to drain measurably faster because of it.** The brief's
premise that "every queued run is serialised behind every other run's
compilation, and with eight lanes editing Rust concurrently that is not a small
term" is, measured, a small term.

### Where the hold actually is, at today's N

| phase | at N=113 | share |
|---|---:|---:|
| reference recording | 22.42 min | 48.2% |
| subject recording | 22.83 min | 49.1% |
| everything else, build included | 1.22 min | 2.6% |

**97.4% of the hold is two recordings**, one of which is branch-invariant. That
is the whole of the opportunity and it has exactly two addresses:
`ci/reference-cache.sh` for the first half (§4, opt-in, the owner's switch),
and — for the second half — the question §8 raises and does not answer.

### The census's own standing

`n=1` for a full end-to-end hold as this was written, and it accrues without
costing anything: it needs no tree to carry an instrument, which is why it
could measure a run on a branch that predates all of this. Re-read it with
`./ci/corpus-hold-census.sh --report <tsv>` before quoting the 46.47.

---

## 10. The reference cache: what is measured, and the one thing that is not

`RUSTMC_REFERENCE_CACHE=1` is the owner's switch and this branch did not touch
it. This section exists so the decision does not require reading four files.

### Measured

| claim | evidence | N |
|---|---|---:|
| the reference recording is ~half a real hold | `corpus-phase-mtimes` 48.2% at N=113; 49.7% mean | n=7 trees |
| ...and is branch-invariant | `record` drives `127.0.0.1:$VAN_PORT`; no path from `crates/rustmc-server` to `vanilla.json` | — |
| a hit boots no vanilla | `reference-cache-integration.sh`, real harness, stub `start` never reached | 6 cases, in T0 |
| a hit still asserts the recording | same, `assert_recording` on the served file | in T0 |
| every mutated key input misses **and names itself** | `reference-cache-falsify.sh` | 24 cases, in T0 |
| a serve costs nothing | `refcache-serve-bench.sh`: 0.106 s | **N=20** |
| serving cannot manufacture a floor of zero | `corpus-floor.sh` refuses byte-identical runs | 36 cases, in T0 |
| the key is usually unchanged | 86.6% of 1,101 `main` commit transitions; 25 of 44 live branches | — |

### Not measured

**A live gate's hold with the flag on, at N greater than one.** `bothways.sh`
on `refcache-bothways-bench` ran it once. That is still the whole of the open
row and this branch did not close it: the batched chain in `~/rustmc-hold` was
five holds behind at 09:52 and is limited to three.

What *has* changed is that it will close itself. `ci/hold-phases.sh` records
`refcache-hit` / `refcache-miss` on every run, and `./ci/hold-phases.sh report`
prints the hit and miss holds side by side as soon as the log holds both. The
number no longer needs anybody to be watching when it happens.

### The prediction, labelled as one

From the measured parts: a hit removes the reference recording (22.42 min at
N=113) and the vanilla boot, and adds a 0.106 s serve. Against a 46.47 min
hold that is **~23.9 min, about 1.94x on the serialised section.** *This is
arithmetic over measured terms, not an observation*, and §4's instruction
stands: do not quote it as a hold until a gate has run both ways.

### Two things to do on the day the flag goes on

1. **Re-run `ci/corpus-phase-mtimes.sh` and read its skip list.** A served
   reference writes `vanilla.json` in 0.1 s, and averaging that into a
   *recording* rate would understate the rate by however many trees hit. The
   harvester already refuses those rows and says so — that refusal was written
   in anticipation of this and has never yet fired in anger.
2. **Watch the `harness` key component.** It hashes the whole of
   `ci/t2-gameplay.sh`, so any edit to that file — a comment included —
   invalidates every cached reference on the box. This branch's own two-line
   move did exactly that, and the miss correctly named `harness` as the
   component that moved. Narrowing it is a separate decision with its own
   falsifier to write.

---

## 11. §8 dated the port fix to the wrong commit

> **This section's FACTS stand and its CONCLUSION does not.** The dating, the
> guard table and the port derivation below are correct, and §§13-14 are built on
> them. Two things here were superseded the same day: the claim that the
> 2026-08-07 overlap is unconfounded evidence *for* the lock (§13 — its three
> instruments cannot tell a stalled run from a healthy one), and the concession
> that 2026-08-06 *was* confounded by ports (§14 — that incident was fourteen
> hours after the fix). Read §13 and §14 before quoting anything below.

**§8 misdates the port fix, and the consequences run in both directions.**
This section corrects the dating. Nothing here changes the protocol; it changes
what the protocol is resting on.

### The dating error

§8 says `92c47ea3` "introduced the corpus lock **and** `pick_port_pair` in the
same commit". It did not. `92c47ea3` (2026-08-07 08:21:27) touches four files
and adds the lock; in its diff of `ci/t2-gameplay.sh` the whole `--- the ports
---` block, including the sentence "Until 2026-08-06 this file hardcoded 25696
and 25697", arrives as **context**, not as an addition. That sentence is the
tell, and it was read backwards: a comment saying the hardcoding ended on 08-06
is a comment written by the commit that ended it, being carried along by a
later one.

`pick_port_pair` entered in **`36b5f48f`, 2026-08-06 01:19:58**, thirty-one
hours before the lock. The same commit added the boot-failure taxonomy that
makes a port collision return **3** and refuse loudly with the holder named,
rather than spending a JVM startup and reporting a five-line stack tail that
named nothing.

So the confound §8 describes is real, and its window is narrow: it covers the
08-06 incident and closes at 01:19 that morning.

### The incident it does not cover

`ci/corpus-lock.sh`'s header records a second overlap, and it is the stronger
of the two because it was not reconstructed:

> Not reconstructed from the 08-06 incident: **measured on 2026-08-07, on
> unlocked code, while the fix for it was being built.** `~/rustmc-chatcmd`'s
> corpus run and the `plugin-hook` merge gate in `~/rustmc-gate` overlapped
> from 02:02 to 02:35.

02:02 on 08-07 is after 01:19 on 08-06 and before 08:21 on 08-07. That window
is exactly the experiment §8 says nobody has run: **per-tree probed ports in
place, no lock, two concurrent recordings.**

Both trees carried the fix, and the evidence is the reflogs rather than an
inference from the date:

| tree | what it was running at 02:02 | descends from `36b5f48f`? |
|---|---|---|
| `~/rustmc-chatcmd` | `8fbf37f9`, checked out 01:16:40, still HEAD until 02:21:43 | yes, and so does every commit that worktree held that night, back to `fbf0ae61` at 08-06 23:26 |
| `~/rustmc-gate` | `f5dd3a06`, which `work-pluginhook` rebased onto `origin/main` at 01:39:34 and checked out at 01:41:00 | yes |

The gate tree's rebase is the load-bearing row: it took `origin/main` twenty-one
minutes before the overlap began, and `36b5f48f` had been on main for a day.

### The ports those two trees would actually have picked

`pick_port_pair` derives a base from the tree root and then probes:

    base = 26000 + 2 * (md5(root)[:8] % 400)

| root | base | pair |
|---|---|---|
| `/home/sayer/rustmc-chatcmd` | 26694 | 26694/26695 |
| `/home/sayer/rustmc-gate` | 26502 | 26502/26503 |

**192 apart.** Not a near miss, and not a case where the probe had to break a
tie. And had they collided, `36b5f48f`'s exit 3 would have refused one of them
by name before it spent a JVM startup. Neither run refused; both progressed,
to 74/86 and beyond.

### The other two explanations are excluded for that night too

* **A shared `oracle-work`.** This is the 2026-08-03 defect that produced
  "12/55 with 11 regressed" for a branch that was not broken, and it is a
  better story than the port one, because one `$VAN_DIR` shared by two runs
  would give exactly the observed silence. It is excluded: the preflight that
  refuses an `oracle-work` symlink pointing outside `$ROOT` landed in
  `5534bd99` on 2026-08-03, and it is present in both `8fbf37f9` and
  `f5dd3a06`.
* **A shared console fifo.** `--console` would put two recorders' gamerule
  writes into one fifo with two readers, and a command delivered to the wrong
  server is a recorder that waits forever. It is excluded by date: `--console`
  is absent from both commits. It entered later.

### What this settles, and the one thing it does not

Settles: **the lock is not resting on a single confounded incident.** There is
one observation, with three independent instruments agreeing, of two concurrent
corpus recordings on per-tree probed ports with no lock, in which each
`oracle-runner record` accumulated 1-2 CPU-seconds in 17 minutes and each
vanilla sat at two client joins for 7+ minutes with no writes. §8's closing
sentence, that "nobody has run two corpus recordings concurrently since
per-tree ports existed, so there is no measurement either way", is false.
Somebody did, on 2026-08-07, by accident, and wrote it down.

Does not settle: **the mechanism.** §8's objection to the accepted account
survives its own dating error and is the sharpest thing in this document:

> ~1 scenario / 10 min | requires a serialising resource nobody has named

That objection is correct and is not answered by the above. Two runs asleep in
their own `settle_ms` timers sleep *in parallel*; parallel sleeping is free,
and it cannot produce a 60x rate collapse. Whatever happened on 08-07 needs a
resource that serialises, and no such resource has been named in this repo. So
what the 08-07 record gives is a **positive control to try to reproduce**, not
a mechanism, and it is N=1, observational, and narrated in a shell comment
rather than captured in a log.

§12 runs the experiment against it.

---

## 12. The exclusion experiment, pre-registered before it ran

Written and committed before a single measurement was taken, because the
result is worth nothing otherwise: an experiment whose design is settled after
its first numbers are seen is a search for a number, and this document has
already retracted one figure this week.

### What is being tested

§8 asked whether two concurrent recordings with per-tree ports produce the same
results as two serialised ones. §11 changes the standing of that question: the
2026-08-07 record is a real, unconfounded observation of the concurrent case,
so this is now an attempt to **reproduce a positive control**, not a walk into
the unknown. The distinction matters for how a null is read, and it is stated
in "what a null would and would not mean" below.

### The rig

* Two scratch trees, `~/rustmc-lockwhy-a` and `~/rustmc-lockwhy-b`, built from
  this branch. Neither is a gate directory and neither is in the queue.
* Port pairs **27100/27101** and **27200/27201**, set through the harness's own
  `T2_GAMEPLAY_VANILLA_PORT` / `T2_GAMEPLAY_RUSTMC_PORT` overrides. Both sit
  **outside** `pick_port_pair`'s 26000-26999 window, so no production tree on
  this box can derive a colliding pair while the experiment is running. The
  experiment cannot become the incident it is studying.
* A **private `RUSTMC_CORPUS_LOCK_DIR` per tree**. Two private namespaces means
  neither run takes the real queue slot, and neither excludes the other. This
  is the lock's own supported environment variable: no tracked file is edited,
  `corpus_lock_acquire` is not stubbed, and the production lock at
  `~/.rustmc-corpus-lock` is untouched throughout.
* `ci/t2-gameplay-subset.sh` over a fixed set of **8 checks**, all of which
  carry a `settle_ms`, since the settle waits are what the lock's header names
  as the thing that collides. The subset harness is the same instrument as the
  full runner: real vanilla as the reference, real server as the subject, real
  recorder. It is roughly one minute rather than forty, which is what makes an
  N worth quoting affordable at all.

**No score from this rig will ever be quoted as a corpus figure.**
`ci/t2-gameplay-subset.sh`'s header forbids it and is right to: its denominator
is whatever was passed on the command line. What is quoted here is a **rate**
and an **agreement**, neither of which is a parity number.

### The two arms, and why they alternate

* **SERIAL**: tree a records, finishes, then tree b records.
* **CONCURRENT**: both launched together.

They **alternate** — S, C, S, C — rather than running as two blocks. This box
reads load 4 and load 110 within the same hour, and `readiness-falsify` was RED
at 11.8 and GREEN at 3.5 on an identical tree. Blocking by arm would confound
the treatment with whatever the box happened to be doing, and the effect being
looked for is a slowdown, which is exactly what load also produces. Every run
records the one-minute load average at its start and at its end.

### A concurrent pair that did not overlap is discarded, not counted

Launching two runs together does not make them concurrent: one can finish its
recording before the other starts its own. Each run's log is timestamped per
line, the reference-recording window is read from it, and the **overlap in
seconds** is computed. A CONCURRENT pair with no overlap of the recording
windows is reported as discarded and excluded from the arm. Counting it would
be a green that cannot go red, which is the defect class this repo names first.

### The two measurements

1. **Rate.** Wall seconds per scenario of the reference recording. This is what
   the lock's founding claim is about: "throughput fell to roughly one scenario
   per ten minutes" against a solo figure of about 9 s per scenario.
2. **Integrity.** `ci/corpus-floor.sh` over each arm's ensemble of
   `vanilla.json` recordings. The floor already exists to say how much two
   independent vanilla runs disagree with each other for reasons of vanilla's
   own non-determinism. **A concurrent pair that disagrees within the serial
   arm's floor is a pair that did not interfere**, and a concurrent arm whose
   floor is higher than the serial arm's is interference.

The second measurement is the one that matters for correctness, and it exists
because a distribution difference is not a divergence. Vanilla's harvest is
non-deterministic on its own, and `-Dmax.bg.threads=1` does not clamp it — that
was refuted in the nether today, where the clamped spread came out *wider*
(34/438/460 against 75/134/151) because `Util.makeExecutor` passes the value as
`ForkJoinPool` parallelism and not as a thread cap. So "the two recordings
differ" is the **null hypothesis here, not evidence of interference**, and the
serial arm is what measures how much they differ anyway.

### Pre-registered predictions

Stated now, so that neither outcome can be presented as the one that was
expected:

1. If the mechanism in `ci/corpus-lock.sh`'s header is real and still present,
   CONCURRENT seconds-per-scenario should exceed SERIAL by a **large multiple**.
   The founding claim is about two orders of magnitude; anything above ~3x
   would be visible at the N this rig can afford.
2. If exclusion is required for correctness rather than for speed, the
   CONCURRENT arm's floor should exceed the SERIAL arm's floor, or a concurrent
   run should refuse outright.
3. The null is that CONCURRENT rate sits inside the SERIAL arm's own spread and
   the two floors are indistinguishable.

### What a null would, and would not, mean

Would: the rate collapse of 2026-08-06 and 2026-08-07 **does not reproduce**
on today's tree at this scale, and the lock's founding justification therefore
does not describe the code as it now stands.

Would not: that the lock should be removed. Three reasons, all of which survive
a null and are the reason this lane recommends rather than flips:

* the subset boots the same two servers but drives 8 scenarios, not 86, and
  holds far less world state. A resource that only serialises under a long run
  would not show here;
* §11's positive control is N=1 and its mechanism is still unnamed. Failing to
  reproduce an effect whose mechanism nobody has identified is weaker evidence
  than failing to reproduce one that has been explained;
* the lock also serialises the *gate*, and gate ordering is a separate property
  from recording integrity. Nothing here measures it.

### Addendum, written after §§13-14 and before any data was kept

Two things above were true when this was committed and are not true now, and
**the design was not changed to suit either of them.** They are recorded here
rather than edited out, because a pre-registration that gets tidied up after the
fact is not one.

1. This section calls the 2026-08-07 record "a real, unconfounded observation of
   the concurrent case" and treats the experiment as an attempt to reproduce a
   **positive control**. §13 withdrew that: the overlap's three instruments read
   the same on a healthy run, so there is no positive control to reproduce. The
   experiment is therefore weaker evidence than it would have been — a null now
   agrees with the historical record instead of contradicting it, which is a
   less demanding test. That cuts against this lane's conclusion and is stated
   for that reason.
2. It says §11's mechanism "is still unnamed". §14 names it: `settle`'s 4 s cap.
3. It describes the eight checks as "all of which carry a `settle_ms`". That is
   true of the *window* and misleading about the *files*: only three declare one
   (`070` at 2000 ms, `100` at 1500 ms, `090` at 1000 ms) and the other five take
   `default_settle()` = **750 ms** from `tools/oracle-runner/src/scenario.rs`.
   The declared plus defaulted total is **8.25 s across the eight**, about 1.03 s
   per scenario against a measured 11.88 s per scenario serialised. Checked after
   the fact and corrected here; the selection was not changed, and it does not
   affect the design, because §14's mechanism is the `settle(QUIET, QUIET_CAP)`
   quiescence wait — which runs twice per scenario whatever `settle_ms` says.

**One change WAS made to the rig after this was written**, and it was a defect
fix rather than a design change. The timestamper was
`stdbuf -oL awk '{print systime(), $0}'`, which sets awk's *output* buffering
and does nothing to its input, so mawk processed a run in one burst and stamped
every line with one time. It was found by inspection during the first smoke run,
before any measurement was kept, and replaced with an unbuffered `read` loop;
`ci/corpus-exclusion-measure-falsify.sh` case 9 now guards it and was shown red
against the old stamper (1 distinct stamp, span 0 s) and green against the new
one (3 stamps, span 4 s). **No arm, metric, discard rule or prediction above was
altered**, and the smoke-run data taken with the broken stamper was discarded
rather than analysed.

---

## 13. The 2026-08-07 evidence is one snapshot, and the run it calls stalled finished a clean corpus

**This withdraws §11's conclusion and replaces it.** §11's facts stand — the
dating, the guards, the port derivation — and they are what make this section
possible. What does not stand is §11's reading of the 08-07 overlap as
unconfounded evidence *for* exclusion. The primary artefacts of that night are
still on this box, and they say the opposite.

### The artefacts

Nothing here is reconstructed. Four files, all with their original mtimes:

| file | mtime | what it is |
|---|---|---|
| `logs/chatcmd/gate-check-before.txt` | 01:44:17 | the box, checked for harnesses, immediately before the run |
| `logs/chatcmd/t2-gameplay.log` | 02:19:51 | the run's own stdout, 39 KB, every `recorded N/88` line |
| `logs/chatcmd/gate-check-after.txt` | 02:19:51 | the box, checked again immediately after |
| `gatelock-logs/live-overlap-evidence.txt` | 02:10:43 | **the whole of the evidence the stall diagnosis rests on** |

The last one is the important one, and it is short enough to quote entire:

    2026-08-07T02:10:43+01:00
    load: 0.06 1.46 2.03 1/537 524028
    oracle-runner pid=444118 cputime=00:00:01 state=wait_woken tree=rustmc-gate
    oracle-runner pid=447613 cputime=00:00:01 state=wait_woken tree=rustmc-chatcmd

That is **one sample, at one instant**. `ci/corpus-lock.sh`'s header renders it
as "each `oracle-runner record` accumulated 1-2 CPU-seconds in 17 minutes",
which is a rate; the file records a cumulative `ps` figure at one moment, at
one-second resolution, and no second sample to difference it against.

### None of the three instruments can tell a stalled run from a healthy one

This was never checked against a run that was known to be fine, and it is the
whole hole. `ci/corpus-healthy-baseline.sh` attaches to whichever run currently
holds the corpus lock — a run that by construction has the box to itself — and
reads exactly those numbers off it. Sampled 2026-08-18 against the
`rustmc-corpusgap` holder, which was at that moment recording normally:

| instrument | 08-07, called a stall | a healthy solo holder, N=34 over 17 min |
|---|---|---|
| `oracle-runner` cputime, as `ps` prints it | `00:00:01` | `00:00:01` at 6m14s; **4.32 s at 1227 s** |
| recorder state | `wait_woken` | `wait_woken` — it is what `pump_both` does |
| vanilla log | 2 joins, no writes for 7+ min | 2 joins, **byte-static at 4297 for 1230 s** |

Sampled every 30 s against the `rustmc-corpusgap` holder from 10:19 to 10:35 on
2026-08-18, every row attributed to the holder's own recorder and none to any
other process on the box. The run was progressing normally throughout and
finished its corpus afterwards.

**A second holder, sampled independently.** `rustmc-proto`, N=24 over 12
minutes on the same day: **2.49 CPU-seconds at 658 s elapsed**, vanilla log
**static for 661 s**, and — the useful difference — **5 joins, not 2**, because
that run had a scenario error and `record` rebuilds its party after one. So the
joins count is not even constant across healthy runs; it counts logins, and a
reader who treats it as progress will misread both directions.

**Read the middle row twice.** The 08-07 evidence for a stall is a recorder
that had used one CPU-second. A recorder that is unambiguously fine uses 4.32
CPU-seconds across twenty minutes, and passes through `00:00:01` on its way
there — so the snapshot says the recorder was early in its run, not that it was
stuck. And the bottom row: 20.5 minutes with the vanilla log not growing by a
single byte is what healthy looks like, against the "7+ minutes" that was cited
as evidence of a stall.

The joins figure is not a throughput counter and cannot be one. `record` logs
its party in **once per run** and reuses it — `tools/oracle-runner/src/record.rs`
fills `party` on the first scenario and only rebuilds it after a scenario
*errors* — so a successful 88-scenario recording produces exactly two
`joined the game` lines, at minute 1 and at minute 40 alike. The control run
above read **2 joins while its own counter stood at 14/108**. Reading "2 client
joins" as "2 scenarios" reads a constant as a rate.

The CPU figure is the same kind of mistake. A recorder spends the corpus inside
`pump_both`, which blocks in 25 ms slices; `ci/corpus-lock.sh`'s own header says
a corpus run "sits in `settle_ms` waits and vanilla boots rather than on a
core". One CPU-second is what healthy looks like.

Only the load average differs between the two columns, and load is a property
of the box, not of a run. Even there the quoted line undercuts itself: `0.06
1.46 2.03` is a one-minute figure of 0.06 sitting under a five-minute figure of
1.46, which is a momentary dip, not fifteen idle minutes.

### The mechanism in the header is not possible

Separately from the evidence, the stated cause does not work. The header says
concurrent runs "each wait out the others' timers". They cannot. The settle
window is

    pump_both(actor, observer, Instant::now() + Duration::from_millis(scenario.settle_ms), ...)

against `while Instant::now() < until`. It is a **fixed deadline**, computed
once. Nothing another process does can extend it, and two runs' settle windows
elapse concurrently rather than end to end. §8 said this claim "requires a
serialising resource nobody has named"; the code says there is no such resource
in the settle path at all.

### The run called stalled completed a full clean corpus

`pid=447613 tree=rustmc-chatcmd`, the second line of the evidence file, is the
recorder of a run that:

* started at **01:44:17** on a box checked and reported `NONE — clear`;
* recorded **88/88 against vanilla** on `:26694` and **88/88 against rustmc** on
  `:26695` — the exact pair §11 derived for that root from the md5, which is
  `pick_port_pair` confirmed against a primary log rather than recomputed;
* finished at **02:19:51** with `CORPUS declared 58/88 match` and
  **`0 vacuous, 0 inert, 0 error`**, against `main`'s baseline of 56/86 with the
  +2 accounted for by the branch's own two new scenarios, so nothing that
  already existed moved;
* and was committed as `108c4ac6`, "all four pre-registered predictions held".

At its finishing instant, `gate-check-after.txt` shows **two other corpus
recordings live on the box**: `~/rustmc-gate`'s runner on `:26503` — the pair
§11 derived for *that* root — already past its reference and recording
`rustmc.json`, and `~/rustmc-gatelock-b`'s on `:26666`.

So the concurrency was real, the ports were per-tree and distinct, and the
output was good.

### The arithmetic that closes it

01:44:17 to 02:19:51 is **2134 s** for two server boots and **176 scenario
recordings**: **12.1 s per scenario including both boots**, against the ~9 s
solo figure the header quotes.

The founding claim is 600 s per scenario. Suppose the whole 02:02 to 02:19:51
overlap, 1071 s, had run at that rate. It could have contained **1.8
scenarios**, leaving 174.2 scenarios for the remaining 1063 s, or **6.1 s per
scenario** — faster than the solo baseline. A collapse of the claimed magnitude
is not merely unobserved on this run; it is arithmetically excluded by the
run's own start and end times.

### What this does and does not establish

Establishes: **the rate claim is refuted for 2026-08-07.** The overlap happened,
with per-tree ports, and one of the overlapping runs completed a full corpus at
a normal rate with a clean result. §8's "there is no measurement either way" is
false — there is one, it is from the incident night, and it favours the null.

Does not establish, and these are the reasons this lane recommends rather than
flips:

* **2026-08-06 is untouched by this.** That incident had shared ports, real
  counter readings (74/86, 50/86, 70/86) and a genuine rate collapse. §8 is
  right that it is confounded; nothing here says it did not happen;
* **integrity is a separate question from rate.** 58/88 with 0 vacuous, 0 inert
  and 0 error is a strong integrity signal from an overlapped run, and it is
  one run. §12's experiment is what puts an N on it;
* **the lock also orders the gate against runs**, which is a property nothing
  in this document measures.

---

## 14. 2026-08-06 was not confounded either, and the only mechanism anybody has named is `settle`'s cap

§8 conceded 2026-08-06 to the port bug and §13 conceded it too. Both were
wrong, and the branch's own commits from that afternoon say so.

### The incident was fourteen hours after the port fix

`36b5f48f` landed at **2026-08-06 01:19:58**. The `movement-tick` worktree's
reflog puts the run and its aftermath in the **afternoon** of the same day:
commits at 16:09:11, 16:11:00 and 16:12:41 record the outcome, and the branch
was first checked out at 14:46. So the three overlapping runs of 2026-08-06 had
**per-tree derived, probed ports**, exactly as 08-07 did. There is no incident
left for the port bug to explain, and `pick_port_pair` is not the answer to any
of this.

That is the second premise of this lane's brief to fall, and it falls the same
way the first did: by reading a commit date instead of a worktree reflog.

### What the surviving run actually did

`docs/intel/movement-tick-corpus-prediction.md` is the primary account of one
of the three, written at the time:

> the **vanilla arm completed, all 86 scenarios**. This server's arm reached
> **74/86 and was then killed**

That is **160 of 172 scenario recordings completed**. And the cause of its
ending is attributed, in `fbf0ae61`, by the person who caused it:

> The corpus run at 74/86 was ended by `pkill -f rustmc-movetick`, run
> deliberately to stop it contending with the `redstone-power` MERGE GATE
> [...] **It was not known that this arm was at 74/86.**

So "one run killed at 74/86 [...] and every one of them ended because a human
noticed" is true in letter and misleading in effect. The human did not notice a
stall. The human killed a healthy run on purpose to protect a gate, and found
out afterwards how far it had got.

### But the rate collapse on 08-06 is real, and it is the one thing that is

`0cd8621d`, same afternoon:

> our arm degraded to roughly one scenario every ten minutes as a second
> agent's full corpus started beside it in `~/rustmc-gate`

That is the harness's own `recorded N/86` counter, watched by the agent whose
run it was, and it is the only reading in this whole affair taken from an
instrument that measures throughput. **It is not confounded by ports and it is
not a proxy.** Whatever else has fallen, this stands: two corpus runs beside
each other on this box, with per-tree ports, and the observed rate fell.

### The mechanism, which is not a serialising resource

§8 said the rate claim "requires a serialising resource nobody has named", and
§13 showed the settle *window* cannot be it, because `settle_ms` is a fixed
deadline. Both are right, and the answer is a different function with a
confusingly similar name.

`tools/oracle-runner/src/client.rs`:

    pub fn settle(&mut self, quiet: Duration, cap: Duration) -> io::Result<()> {
        let deadline = Instant::now() + cap;
        let mut last = Instant::now();
        while Instant::now() < deadline {
            if let Some((id, body)) = self.conn.recv_poll(POLL, PATIENCE)? {
                self.liveness(id, &body)?;
                if !crate::trace::is_liveness(id) { last = Instant::now(); }
            } else if last.elapsed() >= quiet {
                return Ok(());
            }
        }
        Ok(())
    }

with `QUIET = 250 ms` and `QUIET_CAP = 4000 ms`. This is a **resettable**
quiescence wait: every non-liveness packet pushes `last` forward. It is called
**twice per scenario** — `record.rs` lines 447 and 448, inside `run_one`, settle
both sessions after the rig is built and before the measured window opens. (A
third call, line 270 in `warm_up`, is once per run rather than per scenario.)

On a quiet box the server finishes talking and each call returns in ~250 ms. On
a contended box the server's packets arrive late and spread out, `last` keeps
being reset, and the call runs to its **4-second cap**. Two such calls per
scenario is the difference between **~0.5 s and ~8 s of settling per scenario**,
entirely from contention, with no shared resource and no lock anywhere.

**So there is real interference and it is bounded — and the bound is too small
to be the whole story.** `QUIET_CAP` caps the added settling at
2 x (4000 - 250) ms = **7.5 s per scenario**. Against a measured ~12 s per
scenario that is a worst case of about **1.6x**, reached only if *every* settle
runs to its cap on *every* scenario.

That is the right order for `0cd8621d`'s "it slowed rather than sped up" and
for `fbf0ae61`'s "the contention is wall-clock". **It is not the right order for
the tenfold collapse the same commit reports**, and it is nowhere near 600 s per
scenario. So this mechanism is real, it is the only one anybody has named, and
it does not on its own account for the one surviving measurement.

Stating that plainly matters more than closing the gap would. The honest
position is: the 2026-08-06 rate observation is a single contemporaneous
reading, its mechanism is *partly* explained and no more, and §15 reports
whether it reproduces at all on today's tree. An explanation stretched to
cover a number it cannot reach is how the account being corrected here was
built in the first place.

And the measured gap fits it. §15's first pair recorded **11.88 s/scenario
serialised against 12.12 s/scenario concurrent**, a difference of **0.24 s per
scenario** across two `settle` calls — about 120 ms added to each, against a
250 ms floor and a 4000 ms cap. The mechanism is real, it is the right shape,
and at the concurrency this experiment applies it is nowhere near its cap.

### And it is a correctness hazard, not only a slow one

This matters more than the rate. `settle`'s whole purpose is stated in its own
doc comment:

> Waiting for *quiet* instead means the measured window starts when the world
> has actually settled, so a packet caused by rig-building cannot land inside
> it and be attributed to the first real step. That has already happened once.

When the cap is reached without quiet, `settle` **returns anyway** — it is
`Ok(())` on both paths — and the measured window opens on a world that is still
talking. That is precisely the contamination the function exists to prevent,
and contention is what makes it happen. This is the first named mechanism by
which concurrent recordings could corrupt each other's output rather than
merely slow it, and it is the one §12's integrity arm is pointed at.

### Why this is still not an argument for a mutex

The mechanism is driven by **box load**, not by corpus concurrency. Late
packets come from a busy machine, and this machine is busy for reasons that
have nothing to do with the corpus lock: eight lanes compiling Rust, benchmark
sweeps, `gen-gameplay-tables` runs. While the queue described in §15 was five
deep, the one-minute load average on this box read **10 to 25 with the lock
held by a single run**.

A mutex over corpus runs does not lower that load. It serialises the one
activity that is mostly asleep while leaving every CPU-hungry activity on the
box untouched, and charges the queue for it. That is the shape of the
recommendation in §15, and §12's experiment is what decides whether the charge
buys anything.

### The damage tally counts one run twice

`92c47ea3`'s message, and every retelling since, records the cost as:

> One run killed at 74/86, one at 50/86, one abandoned at 70/86.

**74/86 and 70/86 are the same run.** `movement-tick`'s own commits, on the
afternoon this happened:

| time | commit | what it says |
|---|---|---|
| 16:09:11 | `0cd8621d` | "corpus reached **70/86** on our arm, not 41 [...] **The run is still going** under setsid nohup" |
| 16:11:00 | `c5f7a003` | "**The previous two commits said the run was 'still running under setsid nohup'. It is not.**" — found dead at **74/86** |
| 16:12:41 | `fbf0ae61` | the kill was `pkill -f rustmc-movetick`, deliberate, to protect a gate |

Three commits, one hundred and fifty seconds apart, describing one
`rustmc-movetick` run at three moments: a progress reading, its death, and the
attribution of who killed it. The founding message turned the first two into
two separate casualties.

So the tally is **two runs affected, not three**, and of those two the one whose
record survives was not lost to contention at all — it was killed on purpose,
having completed **all 86 vanilla scenarios** and reached 74/86 on the subject
arm.

### And the collapse cannot have lasted long

`work-movetick`'s reflog first checks the branch out at **14:46:05**; the run
was dead by **16:11:00**. That is at most **85 minutes** for **160 completed
scenario recordings** (86 vanilla, 74 subject), or **at most 32 s per scenario
averaged over the whole run** — and less, because the tables and the build come
out of the same 85 minutes.

At the reported collapsed rate of 600 s per scenario, 85 minutes buys **8
scenarios**. So the degradation `0cd8621d` reports is real and it can have
covered **at most 5% of that run's recordings**. The rest ran at a normal rate,
which is the only way 160 of them fit in the window at all.

That is the whole of the surviving quantitative case for exclusivity: a
tenfold degradation, observed once, on part of one run, bounded to at most
about 5% of it by the run's own start and end times.

---

## 15. The result: concurrency costs at most 1.3%, and the cheapest lever needs no code at all

### Lead with what the design excludes, not with the point estimate

A point estimate near 1 is weak on its own — it is what an underpowered
experiment produces when it measures nothing. The useful statement is the
**upper bound**.

N=16 runs per arm, reference-recording phase, seconds per scenario:

| arm | N | mean | sd | median | min | max |
|---|---:|---:|---:|---:|---:|---:|
| serial | 16 | **12.070** | 0.177 | 12.06 | 11.75 | 12.38 |
| concurrent (2 trees) | 16 | **12.109** | 0.151 | 12.12 | 11.88 | 12.38 |

Difference **+0.039 s/scenario**, standard error 0.058, **95% CI −0.075 to
+0.153 s/scenario**. As a ratio that is **0.994x to 1.013x**.

> **This design excludes any concurrent slowdown greater than 1.3%.**

The founding claim is one scenario per ten minutes against a ~12 s baseline —
about **50x**. The experiment excludes that by a factor of roughly **49**. Put
the other way: to detect a 50x collapse you would need N=1, because a single
concurrent run would take fifty minutes instead of one hundred seconds and no
statistics would be involved. Sixteen runs an arm is not what makes this
result; it is what makes the *bound* tight enough to be worth quoting.

### Both load regimes, and the honest gap

Every run recorded the one-minute load at its start and end. Split at the
median:

| band | serial | concurrent | ratio |
|---|---|---|---|
| low | N=8, median 12.12 | N=8, median 12.12 | **1.00x** |
| high | N=8, median 12.00 | N=8, median 12.12 | **1.01x** |

The runs span load **0.1 to 18.3**, and across that 180-fold range the *duration
of an entire run* moved between **204 and 212 seconds** — a 4% spread. A corpus
recording is close to load-invariant in this range, which is the same fact the
lock's own header states from the other side when it says a run "sits in
`settle_ms` waits and vanilla boots rather than on a core".

**The gap, stated rather than glossed: the 50-110 band was not reached.** The
box read load 110 earlier today; during this lane's window it read 0.1 to 18.3,
and a real corpus recording held the lock for most of it. Deliberately loading
the box to 110 would have risked pushing that run's `settle` calls to the cap
described in §14 — the exact contamination this lane exists to study, inflicted
on somebody else's merge. **So this result is reproduced across a 180x load
range and is NOT reproduced at load 110, and that is a real limitation rather
than a rounding of one.** §14's mechanism predicts the ceiling anyway: the most
`QUIET_CAP` can add is 7.5 s on ~12 s, about 1.6x, at any load.

**The arms cannot be load-matched, and the imbalance favours the lock.** Running
two recordings at once *is* an increase in load, so the concurrent arm sits at
higher load by construction. It matched serial from behind.

### Against the box's own history, the concurrent arm is fast

`ci/corpus-phase-mtimes.sh` at **N=113** — real merge gates, each holding the
lock exclusively — gives reference **22.42 min** and subject **22.83 min** over
108 scenarios: **12.46** and **12.68 s/scenario**.

This experiment's *concurrent* arm records at **12.11 s/scenario**. Two
recordings running at the same time are, per scenario, **faster than the
average of 113 runs that had the box to themselves**. Not a matched comparison —
a different scenario mix and a shorter run — so it is a sanity bracket, not a
measurement. But it is the right bracket, and it is on the wrong side for the
lock.

### Which of the four candidate causes actually requires exclusion

| candidate | what already handles it | needs the lock |
|---|---|---|
| **port pairs** | `pick_port_pair` derives per-tree from the root's md5 and probes free; `ci/vanilla-server.sh` returns **3** and refuses by name on a collision before spending a JVM startup | no |
| **shared `oracle-work`** | the preflight refuses a symlink pointing outside `$ROOT` (`5534bd99`); `recording_still_ours` re-checks each recording's `tree=` and `md5=` stamp afterwards and refuses a FOREIGN one — called on **both** recordings, `ci/t2-gameplay.sh:736-737` | no |
| **filesystem cache** | nothing, and nothing needed: both trees sit on one NVMe and share the page cache, which is the production arrangement, and the concurrent arm shows no penalty | no |
| **CPU contention** | nothing — this is the one real term, `settle`'s cap (§14) | **not by exclusion** |

Three of the four are already refused by name, with evidence preserved, by
guards that are not the lock and that fire on every run whether it exists or
not. The fourth is load, and a mutex over corpus runs does not reduce load: it
serialises the one activity on this box that is mostly asleep while leaving
every CPU-hungry activity — eight lanes compiling Rust, benchmark sweeps, table
generation — completely ungoverned.

### What the status quo costs, measured

`./ci/corpus-lock.sh status`, sampled every two minutes for 51 samples across
this lane's window:

* **4.2 runs waiting on average**, maximum 5;
* **431 agent-minutes of accumulated wait** present at any instant;
* the worst single wait reached **174 minutes**.

### THE CHEAPEST LEVER NEEDS NO CODE CHANGE AT ALL

**This is the recommendation the owner should act on first, and it is not this
lane's finding — it is §4 and §10's, made actionable.**

`ci/reference-cache.sh` is built, falsified (6 cases in T0, including that a hit
boots no vanilla and that a served recording still goes through
`assert_recording`), and **default off**. `ci/handoff.sh` already threads the
variable through — lines 764-769 accept `0` or `1`, refuse anything else rather than splicing it into a remote shell, and pass it to stage 6. So the
entire change is:

    RUSTMC_REFERENCE_CACHE=1 ./ci/handoff.sh <branch>

No edit to any tracked file. From §10's measured terms a hit removes the
reference recording (22.42 min at N=113) and the vanilla boot and adds a 0.106 s
serve, taking a 46.47 min hold to **~23.9 min, about 1.94x on the serialised
section**. *That is arithmetic over measured terms and not yet an observed
hold*, and §10's two day-one instructions stand.

**It is a bigger win than anything this lane could propose, it touches no lock,
and it is one environment variable.** Against a queue holding 431 agent-minutes,
halving the hold is worth more than any argument about exclusivity.

### If exclusivity is relaxed later, what changes and what it costs

The mutex is one `flock -x` on `run.lock`. Bounded concurrency is a slot array —
`run.lock.1 .. run.lock.N`, `flock -n` in order, first free slot wins — with the
gate's `gate.lock`, the hand-back, the childless anchor and kernel-owned
liveness all unchanged. That is a small change to `ci/corpus-lock.sh` and **it
is not where the cost is.**

The cost is the falsifiers, and it is the reason not to rush:

* **`ci/corpus-lock-falsify.sh` is 47 cases with 0 injections uncaught**, and
  many assert *exclusion* directly — "4 concurrent runs serialised, no overlap"
  becomes false by construction. Each needs restating as "at most N", plus an
  injection sweep that still catches a lock admitting N+1;
* **`ci/corpus-exclusion-falsify.sh` is 10 cases** whose stated property is
  literally *"while one run is recording, a second run may build, and may NOT
  reach a recording"*. Cases 2, 3 and 6 invert. Its case 7 negative control
  survives unchanged — it already runs two recordings at once;
* `docs/AGENT-BRIEF.md` and `ci/corpus-lock.sh`'s header teach the rule to every
  agent that reads them, and both would be teaching the opposite.

### K=3, the case both incidents actually involved

§15's main result is two concurrent recordings. Both founding incidents were
**three** — 2026-08-06 was "two agents and a merge gate", and at 02:19:51 on
08-07 a third harness was live on `:26666`. A third scratch tree was built and
the rig generalised to N, so the arms are `serial`, `conc2` and `conc3`:

| arm | N | mean s/scen | median | ran at load (median) | vs serial |
|---|---:|---:|---:|---:|---:|
| serial | 4 | 12.219 | 12.12 | 9.0 | — |
| conc2 | 4 | 12.094 | 12.12 | 8.2 | **1.00x** |
| conc3 | 4 | 12.094 | 12.06 | **14.9** | **0.99x** |

95% CI on the ratio: conc2 **0.962x .. 1.018x**, conc3 **0.964x .. 1.015x** —
excluding any slowdown above **1.8%** and **1.5%** respectively. Both point
estimates are at or below 1.

**The `conc3` row is the one to read.** Three recordings at once ran at a median
load of **14.9** against serial's **9.0** — the concurrent arm generated its own
worse conditions, as it must — and still came out **0.99x**. In the high-load
half of the split it is **0.97x**. Three concurrent recordings on this box are
not measurably slower than three serialised ones, at the concurrency and on the
hardware where the founding incidents happened.

Integrity likewise, by the same instrument as §16, and every recording in every
arm byte-distinct from every other:

| arm | recordings | byte-distinct | pairwise | floor |
|---|---:|---:|---:|---:|
| serial | 6 | 6 of 6 | 15 | **0/8** |
| conc2 | 4 | 4 of 4 | 6 | **0/8** |
| conc3 | 6 | 6 of 6 | 15 | **0/8** |

N is small per arm here — this is a confirmation of §15 and §16 at K=3, not an
independent result, and it inherits their limits: eight scenarios rather than
108, and a load band that never reached 110.

### The recommendation

1. **Set `RUSTMC_REFERENCE_CACHE=1`.** One variable, no code, ~1.94x on the
   serialised section, already falsified. Do this first and independently of
   everything else here.
2. **Do not remove the corpus lock**, and nothing in this lane has touched it —
   the only edit to `ci/corpus-lock.sh` is a comment block, and
   `ci/corpus-lock-falsify.sh` is **47/47 with 0 injections uncaught** after it.
3. **Treat bounded concurrency as a live option with a known price**, not as
   settled. The rate evidence for exclusivity is refuted; what is not yet
   measured at any useful N is *integrity* under concurrency at the full
   108-scenario corpus, and the 50-110 load band was never reached.
4. **Retire "one scenario per ten minutes" as a justification.** It is a real
   contemporaneous reading (§14) and it does not reproduce: the same
   arrangement today, with per-tree ports, costs at most 1.3%.

The switch is the owner's. This lane's output is the evidence.

---

## 16. Integrity: the concurrent arm's floor is the serial arm's floor, 0/8 either way

Rate is not integrity, and the 2026-08-06 account made two claims, not one. §15
answers the rate half. This is the other.

`ci/corpus-exclusion-integrity.sh` groups every run's `vanilla.json` by arm and
hands each ensemble to `ci/corpus-floor.sh`, which is the instrument that
already exists for "how much do two independent vanilla runs disagree".

| arm | recordings | byte-distinct | pairwise comparisons | floor |
|---|---:|---:|---:|---:|
| serial | 16 | **16 of 16** | 120 | **0/8** |
| concurrent | 16 | **16 of 16** | 120 | **0/8** |

**The byte-distinct column is the control, and it is why the floor means
something.** All thirty-two recordings differ from one another as files —
vanilla's harvest is non-deterministic in its own right, `-Dmax.bg.threads=1`
does not clamp it (refuted on 2026-08-18: the clamped spread came out *wider*,
34/438/460 against 75/134/151, because `Util.makeExecutor` passes the value as
`ForkJoinPool` parallelism rather than as a thread cap), and two recordings
differing is this experiment's null hypothesis rather than its finding. If the
recordings had come out byte-identical the floor would have been zero by
construction and would have measured nothing; `ci/corpus-floor.sh` refuses that
case outright, and it did not have to.

So: thirty-two files, no two alike, and **every one of the eight scenarios
reproduces in all 240 pairwise comparisons across both arms**. Concurrency
introduced no scenario-level divergence at all, and the arms are not merely
close — they are identical.

### What this bound is, stated as a bound

`ci/corpus-floor.py` prints its own resolving power and it is the honest limit:

> a scenario flapping less than **17.1%** of the time can still show 16
> identical runs (alpha = 0.05)

So this measures: **no scenario diverges more than about 17% of the time under
concurrency that does not also do so serially.** It does not exclude a rare
flap. The floor is a lower bound and grows with N — `docs/PARITY.md` §4 is the
standing example, where the End's floor was 0 across two references and 252
across six — so a larger ensemble could still find something, and neither arm
here is large.

Two further limits, both of which cut against this lane's conclusion:

* **the ensembles are heterogeneous.** Each recording carries its own tree's
  recorder identity, so `corpus-floor` reports 16 recorder groups of 1 and
  **no homogeneity control** in either arm. Nothing here is promotable to a
  standing floor; it is a comparison of two arms measured the same way, which
  is what the question needs, and not a floor anybody should quote;
* **eight scenarios, not 108.** The subset drives the same servers through the
  same recorder, but a longer run holds more world state and more chunks, and
  a divergence that only appears deep into a corpus would not show here.

The `corpus-floor exit: 1` in both arms is the standing `gameplay-floor.toml`
and `gameplay-unstable.toml` declarations disagreeing with an eight-check
subset — expected, identical in both arms, and therefore not something that
discriminates between them.

---

## 17. Pre-registration: integrity at the full 108 scenarios, and the 50-110 load band

**Written and committed before either experiment ran.** §15 and §16 named two
things they did not measure, and they are the whole of this section. Nothing
below is a result; every number here is a design parameter or a prediction.

§16's own statement of its limits is the brief:

> eight scenarios, not 108. The subset drives the same servers through the same
> recorder, but a longer run holds more world state and more chunks, and a
> divergence that only appears deep into a corpus would not show here.

and

> the 50-110 band was NOT reached.

### 17.1 What N buys, stated before the data rather than after it

`ci/corpus-floor.py` prints a per-scenario resolving power, and the arithmetic
behind it is that a scenario which flaps at rate `p` per run shows `n` identical
runs with probability `(1-p)^n`. Setting that to alpha = 0.05,

    per-scenario limit      p = 1 - 0.05^(1/n)

That is the number §16 quoted as 17.1% at n=16, and on its own it is the wrong
figure to design against here, because **this experiment changes S, not n.**
With S scenarios per run the ensemble is silent only if every one of them is
silent, so the quantity that answers "would a divergence anywhere in the corpus
have shown" is

    corpus-wide limit       q = 1 - 0.05^(1/(n*S))

Both are reported, because they answer different questions and the second one
is the one this section exists for.

| design | n | S | per-scenario | corpus-wide |
|---|---:|---:|---:|---:|
| §16, the result being extended | 16 | 8 | 17.07% | 2.313% |
| this section, serial arm | 6 | 108 | 39.30% | **0.461%** |
| this section, concurrent arm | 18 | 108 | **15.33%** | **0.154%** |

So the concurrent arm is tighter than §16 on **both** axes, and the serial
control is 5.0x tighter corpus-wide while being deliberately weaker
per-scenario. **That asymmetry is a choice and it is the one being made
explicit here:** the two arms get equal wall clock, and the concurrent arm
converts equal wall clock into three times the recordings because it runs three
at once. Buying n=18 in the serial arm as well would cost nine further hours to
tighten a control that is already 5x tighter than the published one.

**The N this needs, therefore, is n=6 serial and n=18 concurrent, at S=108, and
that is what is scheduled.** If the concurrent arm shows a divergence the serial
arm does not, n=6 is too small to attribute it and the named scenarios get
re-run at higher n before anything is concluded. That is written here so it
cannot be decided after seeing which way the data fell.

### 17.2 The instrument, and why it is not a new one

`ci/t2-gameplay-selfcheck.sh` already records the whole corpus against a
cold-booted vanilla N times and stamps each recording with the recorder that
made it. It is the reference recording and nothing else, which is the half §2
established is branch-invariant and the half §16 measured, so a run costs about
22 minutes instead of the 45 a subject recording adds. Writing a fresh recorder
would have been about 150 lines that must not drift from `ci/t2-gameplay.sh`'s
substrate, and the drift would be undetectable in the result.

`ci/corpus-integrity-108.sh` is a driver over it and nothing more: it alternates
the arms, gives each tree a private `RUSTMC_CORPUS_LOCK_DIR` and a port base
outside `pick_port_pair`'s 26000-26999, refuses any tree that appears in the
real lock's status, and copies each recording out with its stamp.

**One known difference from the merge gate, constant across arms:** the gate
passes `--console` to `record` and the selfcheck does not, so
`780-random-ticks-turn-covered-grass-to-dirt` (the only check of the 108 with a
`[[gamerule]]` block) is driven differently here. It is driven the same way in
both arms, so it cannot discriminate between them, and it is named rather than
left to be found.

### 17.3 The homogeneity control §16 could not offer

§16 recorded its two ensembles from sixteen different trees and reported
"16 recorder groups of 1 and no homogeneity control", which is why it refused to
promote its floor. The three scratch trees here carry a byte-identical
`oracle-runner` and therefore one recorder identity, `rec:706159cd67984eb2`,
verified before the experiment was scheduled. **Every recording in both arms is
from one recorder**, so `ci/corpus-floor.sh`'s grouping has a single homogeneous
group per arm and a disagreement cannot be a decoder difference.

### 17.4 Predictions, and what would falsify the conclusion

1. **Every recording is byte-distinct from every other.** This is the null
   control, not the finding: vanilla's harvest is non-deterministic in its own
   right and `-Dmax.bg.threads=1` does not clamp it. If any two recordings come
   out byte-identical the floor is zero by construction and measures nothing,
   and the result is withdrawn rather than reported.
2. **The two arms report the same floor.** §16 got 0/8 in both.
3. **A scenario that diverges in the concurrent arm and not in the serial arm
   overturns §15's conclusion**, whatever the rate result says, and is the
   outcome this section is built to be able to produce.
4. A scenario that diverges in *both* arms is vanilla's own variance and is not
   evidence about exclusivity. It would still be worth naming, because the
   standing floor is taken at S=108 and this ensemble is homogeneous.

### 17.5 The load band: why it was refused, and what replaces the refusal

§15's reason for not forcing the 50-110 band was correct and specific: a live
production recording held the lock, and loading the box would have inflicted
§14's `settle`-cap contamination on somebody else's merge.

**That reason is a scheduling constraint, and this box no longer offers a window
to schedule into.** At 13:09 today the lock was HELD by `~/rustmc-hold` with
four runs queued behind it, the oldest waiting 211 minutes. A queue with 211
minutes of backlog does not drain, so waiting for a genuinely idle box is
waiting forever, and "no window appeared" would be a report about the calendar
rather than about the machine.

So the window is constructed rather than found, with three properties, and the
third is what turns an assumption into a measurement:

* **the load runs at `nice 19`, and so do both arms.** CFS weights are
  relative: a `nice 19` recorder among `nice 19` competitors sees the same
  runqueue position it would see as a `nice 0` recorder among `nice 0`
  competitors, which is what reproduces the contention. A production recording
  at `nice 0` has about 68 times the weight of each competitor and is
  substantially insulated from all of them;
* **the load and the arms are pinned with `taskset` to half the box.** The
  production recording is unpinned, so the scheduler has twelve CPUs it can
  place it on that the experiment cannot touch;
* **the production lock holder's own progress is sampled throughout**, by
  `ci/corpus-holder-rate.sh`, before, during and after the load block. §15
  argued the risk of contamination; this measures it. If the holder's
  scenarios-per-second falls during the load block, the block is aborted and
  the reading is reported as a contamination finding rather than a load-band
  one.

**Pre-registered prediction for the load band:** §14's mechanism bounds the
interference term at `QUIET_CAP`, 7.5 s added to a ~12 s scenario, about 1.6x,
**at any load**. So the prediction is a ratio at or below 1.6x, and the
interesting outcome is anything above it, which no named mechanism can produce.
A ratio above 1.6x at load 50-110 would mean the mechanism inventory in §15 is
incomplete, and that is the result worth having.

---

## 18. The falsifier rework, priced: 19 of 33 cases, and six that do not exist yet

§15 priced this in one line, and both halves of it are wrong:

> `ci/corpus-lock-falsify.sh` is 47 cases with 0 injections uncaught
> ...
> `ci/corpus-exclusion-falsify.sh` is 10 cases whose stated property is
> literally *"while one run is recording, a second run may build, and may NOT
> reach a recording"*. Cases 2, 3 and 6 invert.

**47 is not a count of cases.** It is the suite's tally of `ok` assertions, and
`run_suite` is called twice, once as the control before the injection sweep and
once after. Confirmed by running it on this branch's tree on 2026-08-18:

```
cases: 47 passed, 0 failed  (control before sweep: 0 failed)
injections: 0 not caught
CORPUS-LOCK-FALSIFY GREEN
```

**47 PASS lines from 23 case functions run twice.** That run is also this
branch's evidence that it has not touched the lock: `ci/corpus-lock.sh` is
byte-identical to `lockwhy`'s and the suite is green after everything here
landed. The file holds **23 property cases and 19 injections**;
`ci/corpus-exclusion-falsify.sh` holds **10 cases**. Thirty-three cases in total,
not forty-seven, and the difference matters because an estimate quoted in cases
should be a count of the things that have to be rewritten.

**And case 4 inverts too.** It asserts that the lock's own status names the
second run *as a waiter*. Under a lock that admits N, the second run is a
holder, not a waiter, so the assertion is false by construction exactly as 2, 3
and 6 are. §15 lists three and there are four.

### 18.1 The number

`ci/corpus-exclusion-cost.sh` carries the classification as a table, checks it
against both files before printing anything, and **refuses to print a tally if a
case exists that the table does not classify or the table names a case that no
longer exists**. A prose estimate cannot do that and goes stale the first time
somebody adds a case, which is how §15's figure got to be wrong.

```
  existing cases across both falsifiers                     33
    ci/corpus-lock-falsify.sh property cases                23
    ci/corpus-exclusion-falsify.sh cases                    10
    ci/corpus-lock-falsify.sh injections                    19

  A  assertion false by construction, needs a new property  12
  B  assertion stands, setup no longer creates contention    5
  C  assertion stands, target moves to a slot array          2
  D  unchanged                                              14

  CASES NEEDING WORK (A+B+C)                                19
  of which need a property rewritten (A)                    12

  NEW cases required (no coverage today)                     2
  NEW injections required (no coverage today)                4

  TOTAL cases to write or rewrite                           25
```

The change being priced is §15's own sketch and this section does not argue for
it: `run.lock.1 .. run.lock.N`, `flock -n` in order, first free slot wins, with
`gate.lock` still exclusive so a merge gate runs alone.

### 18.2 The four tiers, and why one number would have been dishonest

**A, twelve cases whose assertion is false by construction.** "4 concurrent runs
serialised, no overlap" and "served A B C D" cannot survive a lock that admits N
at once, and neither can the wall-clock floors that go with them: `case_burst`
requires ten one-second holds to take at least ten seconds, which under N=2 is
five. These need a property written, not a string edited.

**B, five cases whose setup stops creating contention.** `case_waits_not_fails`
spawns a holder and three queued runs; under N=4 nothing queues at all and the
case passes while testing nothing. That is the failure mode this repo has most
of, arriving by accident: the assertion is still right and still checks
something real, and it would silently stop being reached.

**C, two cases whose target moves.** `case_anchor_is_sole_holder` counts fd
holders on one lock file and requires exactly one; it becomes exactly one per
slot over N files. Mechanical, but it is the part of those cases that reads
`/proc` and `fuser`, which is where they are hardest to get right.

**D, fourteen cases unchanged.** Anchor liveness, SIGKILL recovery, child fd
discipline, the waiter reaping, and gate-against-gate exclusion do not depend on
how many runs may hold at once. `ci/corpus-exclusion-falsify.sh`'s case 7 is in
here and §15 is right about it: it is the negative control, it already runs two
recordings concurrently, and it survives untouched.

### 18.3 The half an estimate of "rework" leaves out

**All 19 injections stay.** Each of them breaks something that stays broken
under a slot array; what moves is the case that catches it, not the break.

But bounded concurrency creates failure modes that **cannot exist today**, so
nothing in either suite covers them and no existing case would go red:

* **a lock that admits N+1.** The property replacing exclusion needs its own
  case, or the bound is asserted nowhere at all;
* **a slot that leaks.** An array that never marks a released place free
  degrades to N-1, then to 1, silently. Today that failure is a wedge and the
  suite catches it loudly; under an array it is a throughput regression nobody
  sees;
* four injections to go with them: the loop bound off by one, a place never
  released, an ignored `flock` return so two runs land in one place, and places
  picked at random so the array never fills and arrivals queue behind a free
  one.

**So the honest figure is 19 rewrites plus 6 new, and the 12 in tier A are the
ones that need thought rather than typing.** The lock change itself is, as §15
says, small.

---

## 19. A correction to §17.1, made before the data was read

§17.1 pre-registered n=6 serial against n=18 concurrent and called the
asymmetry a choice about wall clock. It is that, and it is also **a confound I
had not priced**, found while block 1 of the experiment was still recording and
therefore before any of its recordings had been looked at:

> **the floor is monotone increasing in n.**

More recordings is more chances to catch a scenario flapping, and nothing else
has to change. `docs/PARITY.md` §4 is the standing example, where the End's
floor was 0 across two references and 252 across six. So an n=18 concurrent arm
would be expected to report a **higher floor than an n=6 serial arm with no
interference at all**, and §17.4's prediction 2, "the two arms report the same
floor", was the wrong prediction to have made.

### 19.1 Measured on this corpus rather than argued from another one

`~/rustmc-items/oracle-work/gameplay-selfcheck/` holds six full-corpus vanilla
reference recordings from 2026-08-13, one recorder, taken serially by
`ci/t2-gameplay-selfcheck.sh`. That is an **external serial control at exactly
the size and shape of this experiment's serial arm**, and it cost nothing but
the projections:

| ensemble | n | floor | scenarios |
|---|---:|---:|---:|
| items selfcheck, all six | 6 | **5** | 108 |
| all 20 three-run subsets of it | 3 | min 3, median 4, max 5 | 108 |

and the per-scenario view of the same subsets shows the mechanism directly:

| seen not reproducing, in what fraction of n=3 subsets | scenario |
|---:|---|
| 100% | `spawn-egg-makes-a-mob` |
| 100% | `dropping-an-item-throws-it-on-the-floor` |
| 100% | `a-dropped-item-goes-into-your-inventory` |
| 80% | `a-player-that-leaves-the-ground-is-resynced` |
| 50% | `a-daylight-detector-inverts-when-right-clicked` |

Halving n did not change what vanilla does. It changed how much of it was
visible, and it moved the headline number from 5 to a median of 4.

### 19.2 What the comparison is instead

`ci/corpus-floor-subsample.py` rewrites `ensemble.json` over subsets of an arm
and re-measures. `ci/corpus-floor.sh` already writes one projection per
recording, so a floor over any subset is arithmetic on files that exist rather
than another twenty-two minutes of vanilla per recording.

So the concurrent arm is read twice:

1. **at matched n.** Draw subsets of size 6 from the concurrent arm's 18,
   measure the floor of each, and ask where the serial arm's single floor sits
   in that distribution. This is the comparison that can be made fairly, and it
   is the one that decides the question;
2. **at its own size**, n=18, which is more sensitive and is not comparable to
   the serial arm's number. It is reported for its resolving power, not for the
   comparison.

**And the scenario-name discriminator is unaffected by any of this**, which is
why it was built first: a scenario that fails to reproduce in the concurrent arm
and in *no* subset of the serial arm is interference whatever the counts do.
`ci/corpus-integrity-arms.sh` prints that set, and §17.4's prediction 3 stands
unamended.

### 19.3 The prediction that replaces §17.4's second one

**Prediction 2, restated:** the serial arm's floor sits inside the middle 90% of
the concurrent arm's matched-n subsample distribution, and the concurrent arm's
floor at n=18 exceeds its own floor at n=6 for the reason above rather than for
any reason about the lock.

Both halves of that can fail, and the first one failing is a result.

---

## 20. The load axis, pre-registered before the loader reported

**Written at 14:30 on 2026-08-18, with `ci/corpus-load-band.sh --calibrate 40`
already running and no window of it yet read.** §17.5 pre-registered what the
*arms* should do in the band. This pre-registers what the **loader** should do,
which is a separate and prior question: an experiment that never reached the
band would produce a ratio just as confidently as one that did.

### 20.1 Why the loader is characterised before the arms, and separately

The 50-110 band needs load, not the lock. It was treated as blocked on the
recordings and it was not: only the integrity question is blocked. So the
loader is established first, on its own, against predictions that can refute it.

### 20.2 A run must be able to say it FINISHED

`ci/corpus-load-band.sh` now asserts its own completeness. `--calibrate`
counts the windows it wrote against the number the design asked for and exits
**8** with `CALIBRATION INCOMPLETE` rather than printing a report over a
fragment; `--hold` prints `LOAD-BLOCK-COMPLETE` only if it reached its budget
and `LOAD-BLOCK-INCOMPLETE` otherwise, and
`ci/corpus-load-band-experiment.sh` reads that rather than assuming.

This is not defensive decoration. On 2026-08-18 the vanilla draw oracle was
found to have been **silently aborting for nine days, reporting 3 chunks where
15 were real**. Every reader of it saw a plausible small number and no failure.
A load block that stops early looks exactly like a load block that ran and
found nothing, and the ratio computed over it is publishable and wrong.

### 20.3 Four predictions, and what each one refutes

| | prediction | what it being false would mean |
|---|---|---|
| **L1** | 40 workers over 12 CPUs put the box's one-minute average at **50 or above** in every ON window | the loader does not reach the band and no result from it is a band result |
| **L2** | 95 workers put it at **100 or above** | the same, at the top of the band |
| **L3** | the production holder's delay share, ON minus OFF, stays **under +0.20** at both counts | the `nice 19` plus `taskset` construction does not protect production, the band experiment must not run, and **that reading is itself the answer to §15's refusal** |
| **L4** | the ARMS' own delay share under load is **at or above** the production holder's at the same time | the `nice 19` arms did not feel the contention they were placed in, and any ratio measured from them is vacuous rather than reassuring |

**L4 is the one that matters and the one most likely to fail.** The whole
licence for constructing a window rather than waiting for one is that CFS
positions are relative: a `nice 19` recorder among `nice 19` competitors sits
where a `nice 0` recorder among `nice 0` competitors would. If that is wrong,
the arms are insulated by the same mechanism that protects production and the
experiment measures nothing. `ci/corpus-holder-rate.sh --tree` exists to read
exactly that, and `ci/corpus-delay-curve.py` bins both by box load so they are
compared at equal load rather than at equal wall clock.

### 20.4 What the box already gave for free, and its ceiling

Passive sampling since 14:06, with no load of this branch's making, over the
production lock holder and this branch's own three recording trees:

| box load1 bin | production holder, nice 0 | this branch's arm, nice 0 |
|---|---:|---:|
| 15.0-20.9 | 0.350 | 0.588 |
| 20.9-26.8 | 0.591 | 0.824 |
| 26.8-32.7 | — | 0.858 |
| 32.7-38.6 | 0.699 | 0.937 |

Delay share, median per bin. Two things come out of it before any load is
applied. **The box's own afternoon reaches 38, not 110** — but see §25: two hours later
it reached **62 unaided**, so that ceiling was a statement about one hour and
not about the machine. And **a
recording that is one of three concurrent ones waits substantially more than a
solo production holder at the same box load**, which is the concurrency term
made visible for the first time: §15 measured that it costs at most 1.3% of
wall clock, and this is the reason it costs anything at all.

### 20.5 One thing this calibration does that was not planned

It is running **inside block 1's concurrent arm**, which began at 14:06:54.
That is stated rather than hidden, and it has a direction: loading the box
during the concurrent arm and not during the serial one biases the integrity
comparison **towards** finding interference. For a result that is expected to
be null that is the conservative direction, and if any concurrent-only scenario
does appear, the first thing to check is whether its recording overlapped
14:30-14:56. `ci/corpus-overlap-sampler.sh`'s timeline is what makes that
checkable.

### 20.6 A caveat on when these samples were taken

The gate chain now **yields the corpus lock between branches**, 45 minutes per
boundary and about 120 minutes per chain, a change made today because runs were
starving. The box's contention profile this afternoon is therefore not the one
the brief for this lane was written under, and none of the passive numbers
above should be quoted as a standing property of the machine.

---

## 21. L3 failed. A second criterion, pre-registered before its data existed

**Written at 14:38 on 2026-08-18, with `ci/corpus-holder-progress.sh` running.**
Its output file held three rows when this was committed, and **all three were
OFF-window rows with `workers=0`**: the loaded half of the comparison L5 tests
had not been sampled at all, so the comparison could not have been made. Stated
exactly rather than as "no data", because "no data" would have been slightly
false and the point of writing this before the result is that it be checkable.

### 21.1 What the loader did, against §20's predictions

| | prediction | outcome |
|---|---|---|
| **L1** | 40 workers put the box at 50+ | **MET.** Box one-minute average **16.07 -> 62.83**, from 40 `nice 19` workers over 12 CPUs. The band is reachable |
| **L2** | 95 workers reach 100+ | **NOT RUN.** L3 fired at 40, so escalating would have been inflicting more of a harm already flagged |
| **L3** | the holder's delay share, ON minus OFF, stays under +0.20 | **FAILED. 0.5271 -> 0.8114, +0.284** |
| **L4** | the arms feel at least as much delay as the holder | **MET.** At matched box load 66, the arm read **0.936** against the holder's **0.858**. The `nice 19` construction delivers contention to the arms rather than insulating them |

L3 failing is the pre-registered stop, and it is honoured: **the 95-worker block
was not run and the chained high-load arms block was killed before it started.**

### 21.2 But the delay share is not harm, and the same samples say so

The production holder's **CPU time per wall second did not move**. Sampled every
60 seconds across the whole afternoon, including through the load block:

| time | box load1 | holder CPU per 60 s | holder delay per 60 s | delay share |
|---|---:|---:|---:|---:|
| 14:30:27 | 20.25 | 168 ms | 104 ms | 0.382 |
| 14:32:27 | 15.52 | 177 ms | 181 ms | 0.506 |
| 14:33:27 | 24.11 | 148 ms | 258 ms | 0.635 |
| 14:34:27 | 46.57 | 150 ms | 470 ms | 0.759 |
| **14:35:27** | **66.13** | **173 ms** | **1047 ms** | **0.858** |
| 14:36:27 | 45.20 | 158 ms | 171 ms | 0.519 |

Across a fourfold rise in box load the holder's CPU consumption stayed inside
**148-197 ms per 60 s**, the same band it occupied all afternoon, and at the
peak it was **173 ms**, above the median. Its runqueue delay went up tenfold and
**it got the same amount of CPU per unit of wall clock anyway.**

That is not a surprise once stated: a corpus recording spends most of its life
asleep in `settle` waits, so there is room to be pushed back in the queue
without anything being lost. **It is the mechanism underneath §15's finding**
that a recording is close to load-invariant, and §15 reports the consequence
without naming the cause.

**So the criterion I pre-registered is more sensitive than the thing it was
protecting.** Delay share fires when a run waits more; a merge is harmed when a
run *finishes later*. Those are different, and the samples above show them
coming apart by a factor of ten.

### 21.3 The criterion that should have been registered, registered now

I cannot substitute a criterion after seeing which way its predecessor fell,
so L3 stands as failed and the escalation stays cancelled. What is legitimate is
to register a second one **before** the data that tests it, which is what this
is.

> **L5.** With 40 workers loading the box to about 63, the production run's own
> `recorded N/M` counter advances at the same **scenarios per minute** as it
> does with the load off, within 10%.

`ci/corpus-holder-progress.sh` counts that run's own progress lines, at 15
second intervals, through an OFF window and an ON window of the same
calibration. It reads a log file and nothing else.

**If L5 holds**, the load-band arms may be run at 40 workers, which is inside
the 50-110 band, and §15's refusal is answered: the band can be entered without
delaying anybody's merge, and the thing §15 was right to worry about turns out
to cost queue position rather than time.

**If L5 fails**, the load band cannot be entered on this box while a production
recording is live, at any worker count that reaches it, and that is the answer
to the brief's second question. It is a real answer and not an absence of one.

### 21.4 The instrument had to be able to say it finished

`ci/corpus-holder-progress.sh`'s first run wrote **`PROGRESS-INCOMPLETE 0 of
40`** and nothing else, because `$(( $(pgrep -fc ...) / 2 ))` is a trap:
`pgrep -c` prints `0` *and* exits 1 when it matches nothing, so the `|| echo 0`
fallback appended a second zero and the arithmetic expansion got `0\n0`. The
loop died before its first row.

**Without the completeness assertion that failure was invisible.** An empty
sample file from a sampler that never ran, and an empty sample file from a run
that made no progress, are the same bytes. That is the shape the vanilla draw
oracle held for nine days while reporting 3 chunks where 15 were real, and it is
why every instrument in this branch now states whether it finished.

---

## 22. §21.2 is wrong, and the load band does cost a recording about 14%

§21.2 argued that the production holder's runqueue delay tripling under load was
not harm, because "its CPU time per wall second did not move". **That is wrong
and this section withdraws it.** It was read off a range rather than a mean, on
a single process, from a sample file that has since been shown to have lost
rows. The properly instrumented answer is the opposite.

### 22.1 What the three concurrent recordings actually did

Three full-corpus recordings, in lockstep at scenario 55/108 when the load
arrived, each sampled every 15 seconds for its own `recorded N/108` counter and
every 60 seconds for its `schedstat`. Loaded means 40 `nice 19` workers pinned
to 12 CPUs, box one-minute average 42 to 66; quiet means the same three
recordings with the load off, box average 11 to 22.

| | loaded | quiet | ratio |
|---|---:|---:|---:|
| arm a, scenarios/min | 4.74 | 5.45 | **0.87** |
| arm b, scenarios/min | 4.74 | 5.50 | **0.86** |
| arm c, scenarios/min | 4.74 | 5.50 | **0.86** |
| arm a, CPU ms per 60 s | 183 | 195 | 0.94 |
| arm b, CPU ms per 60 s | 179 | 194 | 0.92 |
| arm c, CPU ms per 60 s | 181 | 196 | 0.92 |

**A corpus recording in the 50-110 band runs at about 0.86 of its unloaded
rate.** Two independent instruments agree in direction and roughly in size: the
harness's own scenario counter says 14% slower, the kernel's CPU accounting says
6-8% less CPU delivered per unit of wall clock.

So the delay share was not crying wolf. It was the first of three instruments to
notice something the other two then confirmed, and §21.2 talked itself out of
believing it.

### 22.2 What that does to the pre-registered predictions

**L5 fails**, on the arm-side analogue of the quantity it named. It asked for
scenarios per minute unchanged within 10%; the measured change is 14%.

L5's own text says what follows: *"If L5 fails, the load band cannot be entered
on this box while a production recording is live, at any worker count that
reaches it, and that is the answer to the brief's second question."*

**So that is the answer.** §15 declined to force the 50-110 band because loading
the box would contaminate a live production recording. That was not excessive
caution: at the bottom of the band, from a load generator deliberately built to
be gentle — `nice 19`, pinned to half the box, every worker carrying its own
kill deadline — a corpus recording still loses about a seventh of its rate.
§15's refusal is **vindicated on the merits**.

### 22.3 What is measured, and what is still not

**Measured.** The band is reachable (load 16.1 to 62.8 from 40 workers). The
construction delivers contention to the arms rather than insulating them (L4:
arm delay share 0.936 against the holder's 0.858 at matched load 66). And a
recording in the band runs at 0.86 of its quiet rate, on two instruments.

**Not measured, and named rather than glossed.**

* **The serial-versus-concurrent ratio inside the band.** This section says what
  load costs *a* recording. It does not say whether three concurrent recordings
  cost more relative to three serialised ones at load 60 than they do at load
  15, which is the question §15 answered at low load with 0.99x. That needs
  `ci/corpus-load-band-experiment.sh`'s arms, which are the same eight checks
  every run and therefore free of the scenario-mix confound below;
* **the scenario mix is a confound in the numbers above.** The loaded window
  covered corpus scenarios about 56-66 and the quiet window about 67-85, and
  scenarios differ in cost. Three arms agreeing does not fix this: they are in
  lockstep, so they are one measurement, not three. The 0.86 should be read as
  one paired observation with a known bias of unknown sign, corroborated by a
  second instrument on the same runs;
* **the top of the band.** L2 was never run: L3 fired at 40 workers and
  escalating to 95 would have meant inflicting more of a harm already flagged.

### 22.4 The instrument lesson, which cost real data

The passive holder sampler lost its entire first half hour, including every
sample of the run §21.2's withdrawn claim was based on. The cause is the trap
this branch had already written down and then walked into: **`ci/corpus-holder-
rate.sh` was edited twice while a `--follow` instance of it was running.** Bash
reads a script by byte offset, so the running loop executed shifted bytes.

The three `--tree` samplers, started after the last edit, are intact: 36 rows
each, one tree each, no gaps. That contrast is the evidence for the cause.

**The general form is the one this branch keeps meeting.** A file that ends up
holding five rows instead of thirty-five is indistinguishable from a box that
was quiet, unless the instrument states what it expected to write.
`ci/corpus-holder-progress.sh` asserts its row count and stamps
`PROGRESS-COMPLETE`; `ci/corpus-holder-rate.sh` does not, and should.

**Correction, same day: that sentence was too generous to the instrument that
produced this section's own headline.** `ci/corpus-holder-progress.sh` counted
**loop iterations**, not rows that landed, so a run whose every write failed
completed its loop and stamped itself `PROGRESS-COMPLETE 2/2` over a file
holding nothing but a header. Its falsifier, written afterwards, caught it on
the first run, along with a second defect in the fix: `grep -c` prints `0` *and*
exits 1, so a `|| echo 0` fallback produced `0\n0`, the integer comparison
errored, and control fell through to the success branch.

Neither defect touches §22's numbers, whose rows did land and are in the file.
Both are the failure this section is about, found in the instrument this section
is written from, and found only because something was pointed at it:
`ci/corpus-holder-progress-falsify.sh`, 6 cases and 4 injections.

### 22.5 L3's threshold was calibrated against nothing, and L5's was not

§20 set L3's abort line at +0.20 of delay share. **That number was invented
before anything was known about how much the delay share moves on its own**, and
the answer, from this branch's own passive sampling with no load of its making,
is: a great deal.

The production holder's delay share ranged **0.24 to 0.94 across one afternoon**
with nothing of this branch's doing. A +0.20 criterion cannot separate a load
generator from the box's own weather, so **L3 firing is not informative** and its
failure should not be read as "the construction harms production". It should be
read as "this threshold was inside the noise".

**L5's failure is a different matter and it stands.** Scenarios per minute is
not a proxy for anything; it is the quantity a merge waits on. A 14% drop is
14% of somebody's hold, and no argument about thresholds changes it.

So the two predictions fail for different reasons and carry different weight:

* **L3 fails uninformatively.** The threshold should have been set from a
  measured baseline of the quantity, which is exactly what
  `ci/corpus-delay-curve.py` now produces and what a future lane should use;
* **L5 fails informatively**, on two instruments, and is the finding.

**What follows for the remaining measurement.** §22.3 names the one thing still
unmeasured: whether concurrency costs *more* inside the band than outside it,
which is the precise form of the brief's question and the only thing that could
move §15's 1.3%. That needs the arms, and the arms cost the current lock holder
about 14% of however long they run. That is a real price, it is small and
bounded, and it should be stated in minutes rather than argued about in
thresholds: **two reps of `ci/corpus-exclusion-experiment.sh` is about forty
minutes of load, so about six minutes added to one production hold.** Against a
queue that has parked runs for 206 minutes today, that is the cheapest way to
answer the question that is left.

---

## 23. Rate at the full corpus: K=3 costs 0.7%, measured against its own upward bias

§15's rate result is eight scenarios per run. This is the same comparison at
**108**, falling out of the integrity experiment for free because both arms are
timed.

### 23.1 Block 1

| arm | tree | 2 runs | per run |
|---|---|---:|---:|
| serial | lockwhy-a | 43.7 min | **21.83 min** |
| conc3 | lockwhy-b | 44.0 min | 21.98 min |
| conc3 | lockwhy-c | 44.0 min | 21.98 min |
| conc3 | lockwhy-a | 44.0 min | 22.00 min |

**Three concurrent full-corpus recordings: 21.99 min/run against a serial
21.83. Ratio 1.007.** Both arms ran at box loads that overlap (serial 17.0 to
20.8, concurrent 20.8 down to 16.3), and the concurrent arm generated its own
higher load by construction, as §15 notes it must.

### 23.2 The bias in this number points the wrong way for concurrency, which is
### what makes it worth quoting from one block

**About twelve of the concurrent arm's forty-four minutes were deliberately
loaded by this branch**, by the `--calibrate` block described in §20.5 and §22:
40 workers, box average up to 63, inside the arm and not inside its serial
control. §22 measures what that costs a recording at about 14%.

So the 1.007 is an **overestimate of the concurrency penalty**, and the
unloaded figure is lower. A number biased against the conclusion it supports is
worth more from one block than a clean number would be.

It is one block: serial n=2 against concurrent n=6, no confidence interval, and
blocks 2 and 3 will extend it to n=6 and n=18 as §17.1 planned. What it already
does is put the full-corpus figure **inside §15's 1.3% bound rather than
outside it**, which is the thing that could have overturned §15 and does not.

### 23.3 And it is the same instrument reporting both halves

The scenario counter that gives §22 its 14% and the invocation clock that gives
this section its 1.007 are the same runs. **Load costs a recording about a
seventh of its rate; being one of three concurrent recordings costs it under a
hundredth.** Those are the two terms the corpus lock is asked to govern, and it
governs the small one.

---

## 24. Integrity at 108 scenarios, block 1: the one concurrent-only scenario is an artefact of n, and three instruments say so

Block 1 of `ci/corpus-integrity-108.sh`: serial n=2, concurrent n=6, all eight
recordings byte-distinct, all eight from one recorder `rec:706159cd67984eb2`.
§17.4's prediction 1 holds and §17.3's homogeneity control is real rather than
claimed.

### 24.1 The naive reading, which is wrong

| arm | n | floor | scenarios |
|---|---:|---:|---|
| concurrent | 6 | **5/108** | daylight-detector, dropped-item, leaves-the-ground, dropping-an-item, spawn-egg |
| serial | 2 | **4/108** | dropped-item, leaves-the-ground, dropping-an-item, spawn-egg |

> concurrent-only: **1** — `a-daylight-detector-inverts-when-right-clicked`

Taken at face value that is a scenario which fails to reproduce under
concurrency and reproduces serially, which is precisely what §17.4's prediction
3 says would overturn §15. **It is not that, and this is the case §19 exists
for.**

### 24.2 Three independent reasons it is an artefact of sample size

**One: the concurrent arm itself only shows it half the time at the serial
arm's n.** Subsampling the concurrent arm's six recordings down to n=2, over all
fifteen subsets:

| seen not reproducing, in what fraction of n=2 subsets | scenario |
|---:|---|
| 100% | `dropping-an-item-throws-it-on-the-floor` |
| 100% | `spawn-egg-makes-a-mob` |
| 100% | `a-dropped-item-goes-into-your-inventory` |
| **53%** | **`a-daylight-detector-inverts-when-right-clicked`** |
| 53% | `a-player-that-leaves-the-ground-is-resynced` |

At n=2 the concurrent arm misses it about half the time. The serial arm has
n=2. Finding it in one and not the other is a coin flip, not a signal.

**Two: the serial arm's floor sits at the 47th percentile** of the concurrent
arm's matched-n distribution (min 3, median 4, max 5). Dead centre. At equal n
the two arms are indistinguishable, and **no scenario in the serial arm went
unseen by any subset of the concurrent arm**.

**Three, and this is the one that settles it: the same five scenarios are what a
SERIAL ensemble produced on a different day.** `~/rustmc-items`, 2026-08-13, six
full-corpus reference recordings taken serially by `ci/t2-gameplay-selfcheck.sh`
(§19.1):

```
['a-daylight-detector-inverts-when-right-clicked',
 'a-dropped-item-goes-into-your-inventory',
 'a-player-that-leaves-the-ground-is-resynced',
 'dropping-an-item-throws-it-on-the-floor',
 'spawn-egg-makes-a-mob']
```

**Identical set, identical size, no concurrency anywhere near it.** The
"concurrent-only" scenario is a member of this corpus's serial floor at n=6,
measured five days before this experiment existed.

### 24.3 What block 1 therefore says

**The concurrent arm's full-corpus floor is the serial full-corpus floor.**
Nothing diverges under concurrency that does not diverge without it, at 108
scenarios, at a corpus-wide resolving power of **0.461%** in the concurrent arm
against §16's 2.313%.

It is one block, and the serial arm is n=2 rather than the n=6 §17.1 planned;
blocks 2 and 3 bring it there. The bound quoted here is the one the current n
supports and no tighter.

### 24.4 The near miss is the point

Without §19's matched-n subsampling this section would have reported **"one
scenario fails to reproduce under concurrency and not serially"** and that would
have read as the result that keeps the lock. The floor grows with n; the arms
had unequal n by design; and the naive comparison of two floor counts produced a
false positive on the very first block of real data.

That confound was found and corrected **while block 1 was still recording**,
before any of its recordings had been read. Had it been found afterwards, the
correction would have been indistinguishable from rationalising away an
inconvenient result.

---

## 25. The box supplied the band itself, and blocks 2 and 3 are a free experiment inside it

**Written at 15:32 on 2026-08-18, with block 2's serial invocation still
recording and no duration for it in existence.**

§20.4 said the box's own afternoon reached 38 and that the rest of the axis
"has to be made". Two hours later, with nothing of this branch's doing, it read
**62**:

```
15:25:30  56.76      15:28:33  61.54
15:26:31  60.24      15:29:35  68.97
15:27:32  59.33      15:30:54  67.40
```

`ps` at 15:31 attributes it to other lanes: an `-Xmx8G` shape probe in
`~/rustmc-shape` at 686% CPU, a `javap` extraction, further JVMs in
`~/grit/work-cmd` and `~/rustmc-fluid2`, and a `rustc` on `rustmc-server`.
**None of it is this branch's.** The 38 was one hour's weather.

### 25.1 What that hands this experiment

Block 1 recorded at box load **17 to 21** and produced §23's 1.007. Block 2's
serial invocation is recording at **57 to 69**, and block 3 follows it. So
`ci/corpus-integrity-108.sh`, which alternates serial and concurrent arms
inside each block, is now running **the same comparison in two load regimes**,
one below the band and one inside it, with no load generator and therefore no
cost to anybody's merge.

That is exactly the measurement §22.3 named as the one thing still missing:

> whether three concurrent recordings cost more relative to three serialised
> ones at load 60 than they do at load 15.

### 25.2 Registered before block 2's numbers exist

> **L6.** Block 2's concurrent-to-serial ratio is at or below **1.013**, the top
> of §15's 95% interval. If concurrency costs no more inside the band than
> outside it, the ratio should not move from block 1's 1.007.

**What would refute it:** a block-2 ratio materially above 1.013 while both its
arms ran at comparable load. That would say the corpus lock governs a term that
grows with load, which is the one argument for exclusivity that this branch has
not been able to test and §15 could not either.

### 25.3 The two conditions under which L6 cannot be read

Both are properties of the box rather than of the design, so they are stated now
rather than discovered later:

* **the arms must have seen comparable load.** `runs.tsv` stamps the
  one-minute average at the start and end of every invocation. If the load
  subsides between block 2's serial arm and its concurrent arm, the ratio
  measures the box's afternoon and not concurrency, and it must be discarded
  rather than quoted. This is the same discard rule
  `ci/corpus-exclusion-measure.py` applies to a concurrent pair that failed to
  overlap;
* **blocks must not be pooled across load regimes.** Block 1 at load 20 and
  block 2 at load 60 are two experiments, not six recordings from one. §23's
  figure stays a block-1 figure, and any pooled number would average two
  regimes and describe neither.

---

## 26. L6 is discarded, by the rule §25.3 wrote before the data arrived

Block 2 finished. Its ratio is **0.894** — the concurrent arm ten per cent
*faster* than its serial control — and **that number must not be quoted**,
because §25.3's first discard condition failed.

### 26.1 The arms did not see comparable load

Mean box one-minute average over each arm's own window, from
`ci/corpus-overlap-sampler.sh`'s minute-by-minute trace rather than from the
two endpoint stamps in `runs.tsv`:

| arm | window | mean load | min | max | n | per run |
|---|---|---:|---:|---:|---:|---:|
| b2s serial | 14:50-15:34 | **40.4** | 8.2 | 70.1 | 44 | 21.88 min |
| b2c conc3 | 15:34-16:13 | **33.0** | 25.5 | 72.0 | 38 | 19.56 min |

**The serial arm ran at 22% higher mean load than its own concurrent arm.** It
sat on the rising limb of the box's afternoon and the concurrent arm sat on the
falling one. §22 measures load at about 14% of a recording's rate, so a
seven-point load difference is easily the whole 10.6% gap, and it points in the
direction that flatters concurrency.

So block 2 says nothing about concurrency. §25.3:

> If the load subsides between block 2's serial arm and its concurrent arm, the
> ratio measures the box's afternoon and not concurrency, and it must be
> discarded rather than quoted.

**Discarded.** L6 is not tested by it, in either direction, and the fact that
the discarded number happens to support this lane's conclusion is precisely why
the rule was written down first.

### 26.2 The same check, applied to the blocks that survive

| block | serial mean load | concurrent mean load | which arm was penalised | ratio |
|---|---:|---:|---|---:|
| 1 | ~17-21 (endpoints) | **25.7** | the **concurrent** arm | 1.007 |
| 2 | **40.4** | 33.0 | the **serial** arm | ~~0.894~~ discarded |
| 3 | **3.8** | 5.3 | neither, matched | pending |

Block 1 survives and its bias is the conservative one: the concurrent arm
carried the higher load, so 1.007 is an over-estimate of the concurrency
penalty, which is what §23.2 already said for a different reason.

**Block 3 is the well-matched pair**, at 3.8 against 5.3, and it is matched
because the box went quiet.

### 26.3 The quiet is an artefact of the session, and is disclosed

The box was **idle for roughly ninety minutes** from about 15:35, because this
lane's session hit a limit and the agent driving it stopped. The recordings did
not stop: `ci/corpus-integrity-108.sh` was detached and ran through, which is
why block 3 exists at all. But it means

* **block 3's low load is not the box's normal state**, it is the absence of
  everything else that was running at 15:31, and
* **b3s straddles the transition.** Its window is 16:13-16:57, mean 3.8, but
  min 1.1 and max 32.2: the first minutes of it carried the tail of the
  afternoon. Its concurrent partner, starting 16:57, saw 3.2 to 8.3 and no such
  tail.

That is a smaller discontinuity than block 2's and in the same direction as
block 1's, so block 3 is usable with the mismatch stated. It is not a
substitute for a controlled comparison.

### 26.4 What this leaves, and it is one number

Three blocks of full-corpus recordings have produced **one usable ratio at low
load** (block 1's 1.007, conservatively biased) and **no usable ratio inside the
band**. The natural experiment §25 hoped for did not survive its own discard
rule.

The controlled measurement is therefore still the one that answers it:
`ci/corpus-load-band-experiment.sh`, two reps at 40 workers, the same eight
checks every run so there is no scenario-mix confound and no dependence on the
box's mood, with the arms alternating *inside* a single sustained load block so
both see the same load by construction. It is chained behind block 3 and the box
is now quiet enough that its load will be this branch's own rather than a
competitor's.
