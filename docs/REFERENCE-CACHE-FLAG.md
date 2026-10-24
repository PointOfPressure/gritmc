# `RUSTMC_REFERENCE_CACHE=1` — flag proposal for the owner

**Status: proposed, not flipped. Every prerequisite is now met and the
measurement it was waiting on is in — see §"Ready" at the end.** This document
is the decision record area brief "gate throughput" item 1 asked for. Nobody
has set the flag anywhere it would affect a real merge.

## The one-line ask

Set `RUSTMC_REFERENCE_CACHE=1` **only** on `gate-chain.sh`'s own invocation of
`ci/handoff.sh` — the merge chain's line, currently:

```
( cd "$REPO" && RUSTMC_REMOTE_DIR=rustmc-stack ./ci/handoff.sh "$br" )
```

becomes

```
( cd "$REPO" && RUSTMC_REMOTE_DIR=rustmc-stack RUSTMC_REFERENCE_CACHE=1 ./ci/handoff.sh "$br" )
```

**Nowhere else.** Not as a default inside `ci/handoff.sh` (that would also
change what an agent gets from `./ci/handoff.sh mybranch --dry-run` run by
hand to self-check, or from `ci/t2-gameplay-selfcheck.sh`, where a stale
served reference is the opposite of what a diagnostic run wants), and not as
a box-wide environment variable (would leak into every agent session on the
box, gate or not). `ci/handoff.sh` stage 6 already splices the flag onto the
remote command string per-invocation for exactly this reason — see
`ci/handoff.sh:736-767` — so the chain's own call is the only place that
needs to change. `gate-chain.sh` is untracked and owner-controlled; this
document names the line rather than editing it.

## Prerequisite — MET 2026-08-17

`refcache-root-invariance` **merged**: `8ce434d8 ci: refcache_components hashed
the tree's absolute path, defeating cross-tree sharing`, on `origin/main`
(chain log 17:30:06, `+1 commits total 2122`, T0+T1+T2 green, 0 skips). Without
it `refcache_components`'s `scenarios=` hash covered each scenario file's
absolute path, and every tree on this box sits at a distinct
`RUSTMC_REMOTE_DIR`/worktree path by convention — so the flag would have served
nobody while the box ate 21+ minutes storing entries no other tree could key
into. That is fixed, and §"What was measured" item 4 below is the proof it is
fixed in fact and not only in intent.

## What was measured

**1. Key-falsification.** `ci/reference-cache-falsify.sh`, independently
re-run against `refcache-root-invariance`'s tree: **24/24** (was 22/22 on
main without the root-invariance fix). Covers: content-addressing on the jar,
scenario set, recorder source, harness source, the three `Tables::load`
files, seed and projection id; refusal on truncated/tampered/corrupt entries;
immutability (first write wins); opt-in default-off; and the two new cases —
identical content at two different tree roots computes the *same* key, and a
second, differently-located clone *hits* the first tree's entry. A cache that
never misses is the same disease as a check that cannot go red, so every
mutation case is paired with a control that must still hit.

**2. Both-ways, one real gate.** Branch `refcache-bothways-bench` (main +
`refcache-root-invariance`, `--dry-run` throughout, isolated
`RUSTMC_REMOTE_DIR` so it could not collide with `rustmc-stack`): baseline
(cache unset) ran to a real verdict — **GREEN, T0+T1+T2 green, 0 skips**,
108/108 gameplay scenarios recorded and compared. The scripted second half hit
a real-world snag rather than a design flaw: it ran long enough that `main`
advanced 8 commits underneath the held branch and `ci/handoff.sh`'s own
not-behind-main check correctly refused it (`rc=3`) rather than gating a stale
tip. It was rebased and re-run as the direct miss/hit probe below.

**2a. THE LIVE MISS/HIT PROBE — the number this document was waiting on.**
`/home/sayer/grit/bench-refcache-bw-out/cache-hit-probe.log`, 2026-08-17,
sayer-desktop, T2-only (reusing the already-built binary and populated
`oracle-work`, so it pays no T0/T1 recompile). Both passes reached
**T2-GAMEPLAY GREEN**.

**DO NOT QUOTE THE WALL TIMES, AND THIS IS THE FIRST THING TO SAY ABOUT THEM.**
They do not show a saving and two of them show the cache being *slower*:

| pass | what | wall |
|---|---|---:|
| 1 | miss, stores | 4875s |
| 2 | same-tree hit | **5283s** |
| 3 | cross-tree hit | 3004s |
| 4 | repeat same-tree hit | **5190s** |

The probe ran beside the live production chain, so wall is dominated by
**queueing for the corpus lock**, not by the work. Pass 2 waited 66 minutes for
the lock where pass 1 waited 37. A wall figure taken on a contended box is a
measurement of the queue.

The instrument that answers the question is the log's own
`corpus lock: ACQUIRED as gate after waiting Ns` line, subtracted from wall:

| pass | wall | lock wait | **wall − wait** |
|---|---:|---:|---:|
| 1 — MISS, stores | 4875s | 2277s (37m) | **2598s = 43.3 min** |
| 2 — HIT | 5283s | 3988s (66m) | **1295s = 21.6 min** |

**21.7 minutes saved per gate**, and the hit arm's 21.6 min lands on
`docs/CORPUS-LOCK-HOLD.md`'s independent prediction of ~21.6 min to the minute.

Two honest caveats on that pair, both of which the number survives:

* `wall − wait` is an **upper bound** on the in-lock hold — it also carries
  pre-lock setup and post-lock compare. Both arms are measured the same way,
  so the *difference* is sound even though each level is slightly generous.
* **the hit hold is N = 1.** Passes 3 and 4 were not logged separately, so
  their lock waits cannot be recovered and their holds cannot be computed. What
  supports the single sample is that it is not a bare coincidence: the
  mechanism is visible in the log (`no vanilla boot this run`), and the vanilla
  half was independently measured at 21.3 min / 49.8% at N=106 in
  `docs/CORPUS-LOCK-HOLD.md` before this probe existed. A prediction made in
  advance, a mechanism visible in the log, and one end-to-end confirmation is
  weaker than three samples and much stronger than an assertion.

**2b. Cross-tree, and the artefact is IDENTICAL — the correctness evidence.**
`/home/sayer/grit/bench-refcache-bw-out/cross-tree-probe.log`. Tree
`rustmc-refcache-bw-D`, a **different absolute path** from the `bw-A` that
stored the entry, HIT `bw-A`'s key:

```
reference-cache: HIT ref:b87c9aeaddcd3f2d682c09c8c9c3d0738b97cf0eebdc26faeef610516938a15a
```

and both trees' references are byte-identical:

```
d6e5285d389cd20288378acc814a490797b8a4b4477df57ff11d44b10e0aac6e  .../bw-A/oracle-work/gameplay/vanilla.json
d6e5285d389cd20288378acc814a490797b8a4b4477df57ff11d44b10e0aac6e  .../bw-D/oracle-work/gameplay/vanilla.json
```

**This is the load-bearing result, more than any timing.** A cache that is fast
and serves a different reference is worse than no cache: it would move every
gameplay verdict on the box and nothing would say so. A genuine cross-tree hit
whose served artefact hashes the same as the recorded one is the thing that
makes the flag a throughput change rather than a correctness change.

**2c. Disk growth is not a reason to hesitate.** One entry is **696 KB**
(measured, `~/.rustmc-reference-cache`, 2026-08-18). A thousand distinct keys
is under 700 MB against 192 GB free. There is no eviction policy and at this
size there does not need to be one; this is written down so nobody re-opens it
as an objection.

**3. The in-lock hold prediction (`docs/CORPUS-LOCK-HOLD.md`, measured, not
this document's own work).** At N=106: vanilla reference recording 21.3 min
(49.8%, does not depend on the candidate), rustmc subject recording 21.6 min
(50.2%, depends by construction), compare+floor <0.1 min. A hit removes only
the first — `ci/t2-gameplay.sh`'s mechanism is "no vanilla boots at all" —
so the projected hold is **~21.6 min, not near-zero**. That document's own
§5 names this exact live-hit measurement as the one thing left; this
proposal is waiting on the same number.

## Throughput, corrected twice

`ci/lanes.sh`'s header priced lanes off the *uncached* 38.4/53.9 min split
(1.40x at any N). Two corrections land with this proposal, both already
pushed to `lanes-canonical-guard`:

* **Amdahl off the measured cached split: N=2 ~1.46x, N=3 ~1.72x** — not an
  earlier ~1.54x/~1.89x guess, which assumed the vanilla half was the whole
  win rather than half of it, and not a bigger number than the uncached
  1.40x ceiling might suggest is available. The rustmc subject half is a
  **floor**, not a lever more lanes can touch: `ci/corpus-lock.sh`'s own
  header is explicit that the lock exists for resource contention, not
  correctness, and an overlapped pair of subject recordings measured 2
  scenarios in 11 minutes against ~12s solo. A second lane's subject
  recording cannot run inside a first lane's lock window regardless of how
  many lanes exist.
* **Lanes pipeline, they do not batch.** `RUSTMC_LANE_GATE_CMD` defaults to
  `./ci/handoff.sh %BRANCH% --dry-run` — every train position pays for its
  own full gate, corpus lock included. N lanes buy *parallelism* (up to the
  ~1.46x/1.72x above); they do not amortise the ~21.6 min floor across
  several branches merging together. A batching mode that did — one
  integration commit combining K candidates, one T2 corpus run, one floor
  paid instead of K — is a **different and larger piece of work**: a new
  merge-integration path (distinct from the existing lane speculative-train
  merge, which chains branches serially rather than combining them), plus a
  bisect path for a RED batch, because a corpus regression in an integration
  commit does not by itself say which of K candidates caused it. Naive
  linear bisection costs up to K-1 additional gates on a red batch; the
  corpus lock floor this would be amortising is the exact resource a bisect
  would then re-consume repeatedly. **Recommendation: ship lanes + cache
  first** — both are built, falsified, and (modulo the prerequisite above
  and the flip in this document) ready now — **and treat batching as its own
  item**, scoped and falsified separately, rather than folding it into this
  decision. The ~17h batching figure floated earlier assumed lanes.sh already
  did this; it does not, without new machinery.

  **That recommendation was taken, and batching now exists — 2026-08-18.**
  `ci-gate-batching` built exactly the machinery this paragraph said would be
  needed, including the bisect path for a RED batch, and priced it: width **3**,
  computed rather than chosen, **1.175x / ~7.1 h** on the live queue, with a
  13-wide batch measurably *worse* than serial. It is a separate decision with
  its own checklist in `docs/GATE-BATCHING.md` §6 and its own switch (`--land`,
  off). **The two do not interact and neither blocks the other:** the cache
  removes the vanilla half of one gate's corpus hold, batching amortises what
  is left across several branches. Read them as one saving applied twice, not
  as competing proposals — and note that batching's own arithmetic was priced
  against the *uncached* hold, so enabling the cache first makes batching's
  case better, never worse.

## Ready — what would make it safe to enable, and which of those hold

The area brief asked for exactly this list rather than a flip. Every line is a
condition somebody could have refused the flag on:

| # | condition | state |
|---|---|---|
| 1 | `refcache-root-invariance` on `origin/main` | **MET** — `8ce434d8` |
| 2 | key falsification green, with controls that must still hit | **MET** — `ci/reference-cache-falsify.sh` 24/24 |
| 3 | a genuine **cross-tree** hit, not a same-tree one | **MET** — `bw-D` hit `bw-A`'s key |
| 4 | the served reference **byte-identical** to the recorded one | **MET** — both `d6e5285d…` |
| 5 | both arms reach a real verdict, not just a fast one | **MET** — miss and hit both `T2-GAMEPLAY GREEN`, 108/108 |
| 6 | a measured in-lock saving, against a prediction made first | **MET** — 43.3 → 21.6 min, predicted ~21.6 |
| 7 | corrupt / truncated / tampered entries refused | **MET** — falsifier cases |
| 8 | default off; opt-in per invocation | **MET** — `ci/handoff.sh` splices per-call |
| 9 | disk growth bounded in practice | **MET** — 696 KB/entry |
| 10 | the hit hold measured **more than once** | **NOT MET — N = 1** (see §2a) |

**So the honest sentence is: nine of ten hold, and the tenth is a sample size,
not a doubt about the mechanism.** Item 10 costs one T2 run to close, taken as
`role=run` so a real merge gate still jumps ahead of it; it is not worth
holding an eight-hour-a-day saving for, but it is what an owner should be told
before flipping rather than after.

**If the flag is turned on and something is wrong, the failure is loud, not
silent.** The served reference is content-addressed on the jar, the scenario
set, the recorder and harness sources, the three `Tables::load` files, the seed
and the projection id; a mismatch in any of them is a miss and a fresh
recording, not a wrong answer. The scenario a cache normally has to fear —
serving a stale artefact under a key that no longer describes it — is the one
`ci/reference-cache-falsify.sh` spends most of its 24 cases on.

**The revert is one line**, the same line as the flip, and it takes effect on
the next gate. Nothing persists into a tree: an entry is only ever read into
`oracle-work/gameplay/vanilla.json`, which every run overwrites anyway.

## What is NOT proposed here

Flipping the flag. That is the owner's call. This document's job was to make it
a one-line decision with nothing left to look up, and the line is:

> `gate-chain.sh`'s `./ci/handoff.sh` invocation gains
> `RUSTMC_REFERENCE_CACHE=1`; ~21.7 min off every gate; nine of ten enable
> conditions met, the tenth being a second sample of a number already
> predicted independently and confirmed once.
